/*
 *  This file is part of RustyGet.
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
#include "Options.h"
#include "nzbget_rs.h"

class ParRenamer
{
public:
	void Execute()
	{
		int renamedCount = 0;
		int hasMissedFiles = 0;
		int hasDamagedParFiles = 0;
		nzbget_rs_par_renamer_execute(
			m_infoName ? *m_infoName : "",
			m_destDir ? *m_destDir : "",
			g_Options ? g_Options->GetParIgnoreExt() : "",
			m_detectMissing ? 1 : 0,
			PATH_SEPARATOR,
			this,
			+[](void* rawCtx) noexcept -> int
			{
				return static_cast<ParRenamer*>(rawCtx)->IsStopped() ? 1 : 0;
			},
			+[](void* rawCtx, const char* label, int stage) noexcept
			{
				auto* self = static_cast<ParRenamer*>(rawCtx);
				self->m_progressLabel = label ? label : "";
				self->m_stageProgress = stage;
				self->UpdateProgress();
			},
			+[](void* rawCtx, int kind, const char* text) noexcept
			{
				auto* self = static_cast<ParRenamer*>(rawCtx);
				Message::EKind msgKind = kind == 0 ? Message::mkInfo : kind == 1 ? Message::mkWarning : Message::mkError;
				self->PrintMessage(msgKind, "%s", text ? text : "");
			},
			+[](void* rawCtx, const char* name) noexcept
			{
				static_cast<ParRenamer*>(rawCtx)->RegisterParredFile(name ? name : "");
			},
			+[](void* rawCtx, const char* oldRel, const char* newRel) noexcept
			{
				static_cast<ParRenamer*>(rawCtx)->RegisterRenamedFile(
					oldRel ? oldRel : "", newRel ? newRel : "");
			},
			&renamedCount,
			&hasMissedFiles,
			&hasDamagedParFiles);

		m_renamedCount = renamedCount;
		m_hasMissedFiles = hasMissedFiles != 0;
		m_hasDamagedParFiles = hasDamagedParFiles != 0;
	}

	void SetDestDir(const char* destDir) { m_destDir = destDir; }
	const char* GetInfoName() { return m_infoName; }
	void SetInfoName(const char* infoName) { m_infoName = infoName; }
	int GetRenamedCount() { return m_renamedCount; }
	bool HasMissedFiles() { return m_hasMissedFiles; }
	bool HasDamagedParFiles() { return m_hasDamagedParFiles; }
	void SetDetectMissing(bool detectMissing) { m_detectMissing = detectMissing; }

	static bool ListParredFiles(const char* parFilename, std::vector<std::string>& filenames)
	{
		return nzbget_rs_par_renamer_list_parred_files(
			parFilename,
			MaxIndexParSize,
			&filenames,
			+[](void* rawCtx, const char* name) noexcept
			{
				if (name)
				{
					static_cast<std::vector<std::string>*>(rawCtx)->emplace_back(name);
				}
			}) != 0;
	}

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
