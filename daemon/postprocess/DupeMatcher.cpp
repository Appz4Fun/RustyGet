/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2015-2016 Andrey Prygunkov <hugbug@users.sourceforge.net>
 *
 *  This program is free software; you can redistribute it and/or modify
 *  it under the terms of the GNU General Public License as published by
 *  the Free Software Foundation; either version 2 of the License, or
 *  (at your option) any later version.
 *
 *  This program is distributed in the hope that it will be useful,
 *  but WITHOUT ANY WARRANTY; without even the implied warranty of
 *  MERCHANTABILITY or FITNESS FOR A PARTICULAR PURPOSE.  See the
 *  GNU General Public License for more details.
 *
 *  You should have received a copy of the GNU General Public License
 *  along with this program.  If not, see <http://www.gnu.org/licenses/>.
 */


#include "nzbget.h"
#include "DupeMatcher.h"
#include "Log.h"
#include "Util.h"
#include "FileSystem.h"
#include "Options.h"
#include "ScriptController.h"
#include "Thread.h"
#include "nzbget_rs.h"

class RarLister : public Thread, public ScriptController
{
private:
	DupeMatcher* m_owner;
	int64 m_maxSize;
	bool m_compressed;
	bool m_lastSizeMax;
	int64 m_expectedSize;
	char* m_filenameBuf;
	int m_filenameBufLen;
	BString<1024> m_lastFilename;

protected:
	virtual void AddMessage(Message::EKind kind, const char* text);

public:
	virtual void Run();
	static bool FindLargestFile(DupeMatcher* owner, const char* directory,
		char* filenameBuf, int filenameBufLen, int64 expectedSize,
		int timeoutSec, int64* maxSize, bool* compressed);
};

bool RarLister::FindLargestFile(DupeMatcher* owner, const char* directory,
	char* filenameBuf, int filenameBufLen, int64 expectedSize,
	int timeoutSec, int64* maxSize, bool* compressed)
{
	RarLister unrar;
	unrar.m_owner = owner;
	unrar.m_expectedSize = expectedSize;
	unrar.m_maxSize = -1;
	unrar.m_compressed = false;
	unrar.m_lastSizeMax = false;
	unrar.m_filenameBuf = filenameBuf;
	unrar.m_filenameBufLen = filenameBufLen;

	std::vector<CString> cmdArgs = Util::SplitCommandLine(g_Options->GetUnrarCmd());
	if (cmdArgs.empty())
	{
		return false;
	}
	const char* unrarPath = cmdArgs[0];
	unrar.SetArgs({unrarPath, "lt", "*.rar"});
	unrar.SetWorkingDir(directory);

	time_t curTime = Util::CurrentTime();

	unrar.Start();

	// wait up to iTimeoutSec for unrar output
	while (nzbget_rs_dqh_dupematcher_should_wait_rar_lister(
		unrar.IsRunning() ? 1 : 0,
		(int64_t)curTime,
		timeoutSec,
		(int64_t)Util::CurrentTime()) != 0)					// in a case clock was changed
	{
		Util::Sleep(200);
	}

	if (unrar.IsRunning())
	{
		unrar.Terminate();
	}

	// wait until terminated or killed
	while (unrar.IsRunning())
	{
		Util::Sleep(200);
	}

	*maxSize = unrar.m_maxSize;
	*compressed = unrar.m_compressed;

	return true;
}

void RarLister::Run()
{
	Execute();
}

void RarLister::AddMessage(Message::EKind kind, const char* text)
{
	int compressed = m_compressed ? 1 : 0;
	int lastSizeMax = m_lastSizeMax ? 1 : 0;
	int action = nzbget_rs_dupematcher_parse_rar_lister_line(
		text, m_expectedSize, &m_maxSize, &compressed, &lastSizeMax);
	m_compressed = compressed != 0;
	m_lastSizeMax = lastSizeMax != 0;
	if (action == 1)
	{
		m_owner->PrintMessage(Message::mkDetail, "Reading file %s", text + 9);
	}
	else if (action == 2)
	{
		m_lastFilename = text + 14;
	}
	else if (action == 3)
	{
		strncpy(m_filenameBuf, m_lastFilename, m_filenameBufLen);
		m_filenameBuf[m_filenameBufLen-1] = '\0';
		return;
	}
	else if (action == 4)
	{
		return;
	}
	else if (action == 5)
	{
		Terminate();
	}
}


namespace
{

struct RarListerCtx
{
	DupeMatcher* owner;
	const char* directory;
};

void OnRarArchive(void* rawCtx, char* filenameBuf, int bufLen, int64 expectedSize,
	int64* outMaxSize, int* outCompressed)
{
	RarListerCtx* ctx = static_cast<RarListerCtx*>(rawCtx);
	bool comp = *outCompressed != 0;
	RarLister::FindLargestFile(ctx->owner, ctx->directory, filenameBuf, bufLen,
		expectedSize, 60, outMaxSize, &comp);
	*outCompressed = comp ? 1 : 0;
}

}

bool DupeMatcher::SizeDiffOK(int64 size1, int64 size2, int maxDiffPercent)
{
	return nzbget_rs_dupematcher_size_diff_ok(size1, size2, maxDiffPercent) != 0;
}

bool DupeMatcher::Prepare()
{
	RarListerCtx ctx{ this, m_destDir };
	int comp = m_compressed ? 1 : 0;
	bool sizeOK = nzbget_rs_dupematcher_prepare(
		m_destDir,
		m_expectedSize,
		&m_maxSize,
		&comp,
		OnRarArchive,
		[](void* rawCtx, const char* msg) {
			static_cast<RarListerCtx*>(rawCtx)->owner->PrintMessage(
				Message::mkDetail, "%s", msg ? msg : "");
		},
		&ctx) != 0;
	m_compressed = comp != 0;
	return sizeOK;
}

bool DupeMatcher::MatchDupeContent(const char* dupeDir)
{
	RarListerCtx ctx{ this, dupeDir };
	return nzbget_rs_dupematcher_match_dupe_content(
		dupeDir,
		m_maxSize,
		m_compressed ? 1 : 0,
		OnRarArchive,
		[](void* rawCtx, const char* msg) {
			static_cast<RarListerCtx*>(rawCtx)->owner->PrintMessage(
				Message::mkDetail, "%s", msg ? msg : "");
		},
		&ctx) != 0;
}

void DupeMatcher::FindLargestFile(const char* directory, char* filenameBuf, int bufLen,
	int64* maxSize, bool* compressed)
{
	RarListerCtx ctx{ this, directory };
	int comp = 0;
	nzbget_rs_dupematcher_find_largest_file(
		directory,
		filenameBuf,
		bufLen,
		maxSize,
		&comp,
		maxSize == &m_maxSize ? 1 : 0,
		m_maxSize,
		OnRarArchive,
		&ctx);
	*compressed = comp != 0;
}
