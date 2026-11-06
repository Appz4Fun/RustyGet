/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2013-2016 Andrey Prygunkov <hugbug@users.sourceforge.net>
 *  Copyright (C) 2024 Denis <denis@nzbget.com>
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
 *  along with this program.  If not, see <https://www.gnu.org/licenses/>.
 */


#include "nzbget.h"

#ifndef DISABLE_PARCHECK

#include "ParRenamer.h"
#include "Log.h"
#include "Options.h"
#include "nzbget_rs.h"

void ParRenamer::Execute()
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
		[](void* rawCtx) noexcept -> int
		{
			return static_cast<ParRenamer*>(rawCtx)->IsStopped() ? 1 : 0;
		},
		[](void* rawCtx, const char* label, int stage) noexcept
		{
			auto* self = static_cast<ParRenamer*>(rawCtx);
			self->m_progressLabel = label ? label : "";
			self->m_stageProgress = stage;
			self->UpdateProgress();
		},
		[](void* rawCtx, int kind, const char* text) noexcept
		{
			auto* self = static_cast<ParRenamer*>(rawCtx);
			Message::EKind msgKind = kind == 0 ? Message::mkInfo : kind == 1 ? Message::mkWarning : Message::mkError;
			self->PrintMessage(msgKind, "%s", text ? text : "");
		},
		[](void* rawCtx, const char* name) noexcept
		{
			static_cast<ParRenamer*>(rawCtx)->RegisterParredFile(name ? name : "");
		},
		[](void* rawCtx, const char* oldRel, const char* newRel) noexcept
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

bool ParRenamer::ListParredFiles(const char* parFilename, std::vector<std::string>& filenames)
{
	return nzbget_rs_par_renamer_list_parred_files(
		parFilename,
		MaxIndexParSize,
		&filenames,
		[](void* rawCtx, const char* name) noexcept
		{
			if (name)
			{
				static_cast<std::vector<std::string>*>(rawCtx)->emplace_back(name);
			}
		}) != 0;
}

#endif
