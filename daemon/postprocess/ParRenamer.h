/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2013-2016 Andrey Prygunkov <hugbug@users.sourceforge.net>
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


#ifndef PARRENAMER_H
#define PARRENAMER_H

#ifndef DISABLE_PARCHECK

#include <string>
#include <vector>
#include "NString.h"
#include "Log.h"

class ParRenamer
{
public:
	void Execute();
	void SetDestDir(const char* destDir) { m_destDir = destDir; }
	const char* GetInfoName() { return m_infoName; }
	void SetInfoName(const char* infoName) { m_infoName = infoName; }
	int GetRenamedCount() { return m_renamedCount; }
	bool HasMissedFiles() { return m_hasMissedFiles; }
	bool HasDamagedParFiles() { return m_hasDamagedParFiles; }
	void SetDetectMissing(bool detectMissing) { m_detectMissing = detectMissing; }
	/* Base names of the files a par2-file's description packets list (only
	 * that file's packets are read). False when it cannot be loaded or is
	 * larger than MaxIndexParSize. */
	static bool ListParredFiles(const char* parFilename, std::vector<std::string>& filenames);
	static constexpr int64 MaxIndexParSize = 16 * 1024 * 1024;

protected:
	virtual void UpdateProgress() {}
	virtual bool IsStopped() { return false; };
	virtual void PrintMessage([[maybe_unused]] Message::EKind kind, [[maybe_unused]] const char* format,
		...) PRINTF_SYNTAX(3) {}
	virtual void RegisterParredFile([[maybe_unused]] const char* filename) {}
	virtual void RegisterRenamedFile([[maybe_unused]] const char* oldFilename,
		[[maybe_unused]] const char* newFileName) {}
	const char* GetProgressLabel() { return m_progressLabel; }
	int GetStageProgress() { return m_stageProgress; }

private:
	CString m_infoName;
	CString m_destDir;
	CString m_progressLabel;
	int m_stageProgress = 0;
	int m_renamedCount = 0;
	bool m_hasMissedFiles = false;
	bool m_detectMissing = false;
	bool m_hasDamagedParFiles = false;
};

#endif

#endif
