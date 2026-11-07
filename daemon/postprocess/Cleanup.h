/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2013-2016 Andrey Prygunkov <hugbug@users.sourceforge.net>
 *  Copyright (C) 2025-2026 Denis <denis@nzbget.com>
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


#ifndef CLEANUP_H
#define CLEANUP_H

#include <string>
#include "NString.h"
#include "Log.h"
#include "Thread.h"
#include "DownloadInfo.h"
#include "ScriptController.h"
#include "Options.h"
#include "FileSystem.h"
#include "Util.h"
#include "nzbget_rs.h"

class MoveController : public Thread, public ScriptController
{
public:
	void Run() override
	{
		BString<1024> nzbName;
		{
			GuardedDownloadQueue guard = DownloadQueue::Guard();
			nzbName = m_postInfo->GetNzbInfo()->GetName();
			m_interDir = fs::u8path(m_postInfo->GetNzbInfo()->GetDestDir());
			m_destDir = fs::u8path(m_postInfo->GetNzbInfo()->GetFinalDir());
			if (m_destDir.empty())
			{
				m_destDir = fs::u8path(m_postInfo->GetNzbInfo()->BuildFinalDirName().Str());
			}
		}

		NzbgetRsBuf infoNameBuf = nzbget_rs_tpr_post_format_info_name("move", *nzbName);
		BString<1024> infoName;
		infoName.Set(infoNameBuf.data ? infoNameBuf.data : "", (int)infoNameBuf.len);
		nzbget_rs_free(infoNameBuf);
		SetInfoName(*infoName);
		PrintMessage(Message::mkInfo, "Moving completed files for %s", *nzbName);

		bool ok = MoveFiles();

		nzbget_rs_dqh_post_capitalize_first_ascii(*infoName);

		if (ok)
		{
			RemoveStaleHardlinks(*m_postInfo->GetNzbInfo(), m_destDir);
		}

		{
			GuardedDownloadQueue guard = DownloadQueue::Guard();
			if (m_postInfo && m_postInfo->GetNzbInfo())
			{
				int moveStatus = 0;
				int outcome = nzbget_rs_move_controller_decide_outcome(
					ok ? 1 : 0, IsStopped() ? 1 : 0, &moveStatus);
				if (outcome == 1)
				{
					PrintMessage(Message::mkInfo, "%s successful", *infoName);
					m_postInfo->GetNzbInfo()->SetDestDir(fs::u8string(m_destDir).c_str());
					m_postInfo->GetNzbInfo()->SetFinalDir("");
					m_postInfo->GetNzbInfo()->SetMoveStatus((NzbInfo::EMoveStatus)moveStatus);
				}
				else if (outcome == 2)
				{
					PrintMessage(Message::mkInfo, "%s interrupted, resumes after the restart", *infoName);
				}
				else
				{
					PrintMessage(Message::mkError, "%s failed", *infoName);
					m_postInfo->GetNzbInfo()->SetMoveStatus((NzbInfo::EMoveStatus)moveStatus);
				}
				m_postInfo->SetWorking(false);
			}
		}
	}

	static void StartJob(PostInfo* postInfo)
	{
		MoveController* moveController = new MoveController();
		moveController->m_postInfo = postInfo;
		moveController->SetAutoDestroy(false);

		postInfo->SetPostThread(moveController);

		moveController->Start();
	}

	static Message::EKind ToMessageKind(int kind)
	{
		switch (kind)
		{
		case 1:
			return Message::mkWarning;
		case 2:
			return Message::mkError;
		default:
			return Message::mkInfo;
		}
	}

protected:
	void AddMessage(Message::EKind kind, const char* text) override
	{
		m_postInfo->GetNzbInfo()->AddMessage(kind, text);
	}

	bool MoveFiles()
	{
		const std::string interStr = fs::u8string(m_interDir);
		const std::string destStr = fs::u8string(m_destDir);
		return nzbget_rs_move_controller_move_files(
			interStr.c_str(),
			destStr.c_str(),
			this,
			[](void* rawCtx) -> int {
				return static_cast<MoveController*>(rawCtx)->IsStopped() ? 1 : 0;
			},
			[](void* rawCtx, int kind, const char* msg) {
				static_cast<MoveController*>(rawCtx)->PrintMessage(
					ToMessageKind(kind), "%s", msg ? msg : "");
			},
			[](void* rawCtx, const char* oldRelName, const char* newRelName) {
				auto* self = static_cast<MoveController*>(rawCtx);
				GuardedDownloadQueue guard = DownloadQueue::Guard();
				if (self->m_postInfo && self->m_postInfo->GetNzbInfo())
				{
					self->m_postInfo->GetNzbInfo()->RenameCompletedFile(
						oldRelName ? oldRelName : "", newRelName ? newRelName : "");
				}
			}) != 0;
	}

	void RemoveStaleHardlinks(NzbInfo& nzbInfo, const fs::path& destDir)
	{
		const auto& hardLinkPath = nzbInfo.GetHardLinkPath();
		if (hardLinkPath.empty()) return;

		const std::string destStr = fs::u8string(destDir);
		nzbget_rs_move_controller_remove_stale_hardlinks(
			hardLinkPath.c_str(),
			destStr.c_str(),
			this,
			[](void* rawCtx, int kind, const char* msg) {
				static_cast<MoveController*>(rawCtx)->PrintMessage(
					ToMessageKind(kind), "%s", msg ? msg : "");
			});
	}

private:
	PostInfo* m_postInfo = nullptr;
	fs::path m_interDir;
	fs::path m_destDir;
};

class CleanupController : public Thread, public ScriptController
{
public:
	void Run() override
	{
		BString<1024> nzbName;
		CString destDir;
		CString finalDir;
		{
			GuardedDownloadQueue guard = DownloadQueue::Guard();
			nzbName = m_postInfo->GetNzbInfo()->GetName();
			destDir = m_postInfo->GetNzbInfo()->GetDestDir();
			finalDir = m_postInfo->GetNzbInfo()->GetFinalDir();
		}

		NzbgetRsBuf infoNameBuf = nzbget_rs_tpr_post_format_info_name("cleanup", *nzbName);
		BString<1024> infoName;
		infoName.Set(infoNameBuf.data ? infoNameBuf.data : "", (int)infoNameBuf.len);
		nzbget_rs_free(infoNameBuf);
		SetInfoName(*infoName);

		PrintMessage(Message::mkInfo, "Cleaning up %s", *nzbName);

		int cleanupStatus = 0;
		int cleanupOutcome = nzbget_rs_cleanup_controller_run(
			destDir ? *destDir : "",
			finalDir ? *finalDir : "",
			g_Options->GetExtCleanupDisk(),
			this,
			[](void* rawCtx, int kind, const char* msg) {
				static_cast<CleanupController*>(rawCtx)->PrintMessage(
					MoveController::ToMessageKind(kind), "%s", msg ? msg : "");
			},
			&cleanupStatus);

		nzbget_rs_dqh_post_capitalize_first_ascii(*infoName);

		if (cleanupOutcome == 1)
		{
			PrintMessage(Message::mkInfo, "%s successful", *infoName);
		}
		else if (cleanupOutcome == 2)
		{
			PrintMessage(Message::mkInfo, "Nothing to cleanup for %s", *nzbName);
		}
		else
		{
			PrintMessage(Message::mkError, "%s failed", *infoName);
		}
		m_postInfo->GetNzbInfo()->SetCleanupStatus((NzbInfo::ECleanupStatus)cleanupStatus);

		m_postInfo->SetWorking(false);
	}

	static void StartJob(PostInfo* postInfo)
	{
		CleanupController* cleanupController = new CleanupController();
		cleanupController->m_postInfo = postInfo;
		cleanupController->SetAutoDestroy(false);

		postInfo->SetPostThread(cleanupController);

		cleanupController->Start();
	}

protected:
	void AddMessage(Message::EKind kind, const char* text) override
	{
		m_postInfo->GetNzbInfo()->AddMessage(kind, text);
	}

private:
	PostInfo* m_postInfo;
	CString m_destDir;
	CString m_finalDir;
};

#endif
