/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2016-2019 Andrey Prygunkov <hugbug@users.sourceforge.net>
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
#include "Options.h"
#include "DiskState.h"
#include "Log.h"
#include "FileSystem.h"
#include "Rename.h"
#include "nzbget_rs.h"

#ifndef DISABLE_PARCHECK
void RenameController::PostParRenamer::PrintMessage(Message::EKind kind, const char* format, ...)
{
	char text[1024];
	va_list args;
	va_start(args, format);
	vsnprintf(text, 1024, format, args);
	va_end(args);
	text[1024-1] = '\0';

	m_owner->m_postInfo->GetNzbInfo()->AddMessage(kind, text);
}
#endif


void RenameController::PostRarRenamer::PrintMessage(Message::EKind kind, const char* format, ...)
{
	char text[1024];
	va_list args;
	va_start(args, format);
	vsnprintf(text, 1024, format, args);
	va_end(args);
	text[1024 - 1] = '\0';

	m_owner->m_postInfo->GetNzbInfo()->AddMessage(kind, text);
}


RenameController::RenameController()
	: m_postInfo(nullptr)
	, m_kind(jkPar)
{
	debug("Creating RenameController");

#ifndef DISABLE_PARCHECK
	m_parRenamer.m_owner = this;
#endif

	m_rarRenamer.m_owner = this;
}

void RenameController::StartJob(PostInfo* postInfo, EJobKind kind)
{
	RenameController* renameController = new RenameController();
	renameController->m_postInfo = postInfo;
	renameController->m_kind = kind;
	renameController->SetAutoDestroy(false);

	postInfo->SetPostThread(renameController);

	renameController->Start();
}

void RenameController::Run()
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

	NzbgetRsBuf infoNameBuf = nzbget_rs_tpr_post_format_info_name("rename", *nzbName);
	BString<1024> infoName;
	infoName.Set(infoNameBuf.data ? infoNameBuf.data : "", (int)infoNameBuf.len);
	nzbget_rs_free(infoNameBuf);
	SetInfoName(infoName);

	const char* archiveKindPrefix = nzbget_rs_tpr_rename_ctrl_archive_kind_prefix(
		(int)m_kind, (int)jkRar);
	PrintMessage(Message::mkInfo, "Checking renamed %sfiles for %s",
		archiveKindPrefix, *nzbName);

	ExecRename(destDir, finalDir, nzbName);

	int runOutcome = nzbget_rs_dqh_rename_ctrl_classify_run_outcome(
		IsStopped() ? 1 : 0, m_renamedCount);
	if (runOutcome == 1)
	{
		PrintMessage(Message::mkWarning, "Renaming cancelled for %s", *nzbName);
	}
	else if (runOutcome == 2)
	{
		PrintMessage(Message::mkInfo, "Successfully renamed %i %sfile(s) for %s",
			m_renamedCount, archiveKindPrefix, *nzbName);
	}
	else
	{
		PrintMessage(Message::mkInfo, "No renamed %sfiles found for %s",
			archiveKindPrefix, *nzbName);
	}

	RenameCompleted();
}

void RenameController::AddMessage(Message::EKind kind, const char* text)
{
	m_postInfo->GetNzbInfo()->AddMessage(kind, text);
}

void RenameController::ExecRename(const char* destDir, const char* finalDir, const char* nzbName)
{
	if (m_kind == jkPar)
	{
#ifndef DISABLE_PARCHECK
		int detectMissingInt = 0;
		bool useFinal = nzbget_rs_psp_rename_ctrl_eval_par_rename_config(
			(int)m_postInfo->GetNzbInfo()->GetUnpackStatus(),
			(int)NzbInfo::usSuccess,
			(int)NzbInfo::usNone,
			Util::EmptyStr(finalDir) ? 1 : 0,
			&detectMissingInt) != 0;
		m_parRenamer.SetDestDir(useFinal ? finalDir : destDir);
		m_parRenamer.SetInfoName(nzbName);
		m_parRenamer.SetDetectMissing(detectMissingInt != 0);
		m_parRenamer.Execute();
#endif
	}
	else if (m_kind == jkRar)
	{
		m_rarRenamer.SetDestDir(destDir);
		m_rarRenamer.SetInfoName(nzbName);
		m_rarRenamer.SetIgnoreExt(g_Options->GetUnpackIgnoreExt());

		NzbParameter* parameter = m_postInfo->GetNzbInfo()->GetParameters()->Find("*Unpack:Password");
		if (parameter)
		{
			m_rarRenamer.SetPassword(parameter->GetValue());
		}

		m_rarRenamer.Execute();
	}
}

void RenameController::RenameCompleted()
{
	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();

	int reqExtraPars = 0;
	int reqParCheck = 0;
#ifndef DISABLE_PARCHECK
	int hasDamagedParFiles = m_parRenamer.HasDamagedParFiles() ? 1 : 0;
	int hasMissedFiles = m_parRenamer.HasMissedFiles() ? 1 : 0;
#else
	int hasDamagedParFiles = 0;
	int hasMissedFiles = 0;
#endif
	int initialStatus = nzbget_rs_rco_rename_controller_complete_action(
		(int)m_kind, m_renamedCount,
		hasDamagedParFiles,
		m_postInfo->GetNzbInfo()->GetRemainingParCount(),
		hasMissedFiles,
		(int)m_postInfo->GetNzbInfo()->GetParStatus(),
		&reqExtraPars, &reqParCheck);
	if (m_kind == jkPar)
	{
		m_postInfo->GetNzbInfo()->SetParRenameStatus((NzbInfo::EPostRenameStatus)initialStatus);
#ifndef DISABLE_PARCHECK
		if (reqExtraPars)
		{
			m_parRenamer.PrintMessage(Message::mkInfo, "Requesting extra par2-files for %s to perform par-rename", m_parRenamer.GetInfoName());
			downloadQueue->EditEntry(m_postInfo->GetNzbInfo()->GetId(), DownloadQueue::eaGroupResume, nullptr);
			downloadQueue->EditEntry(m_postInfo->GetNzbInfo()->GetId(), DownloadQueue::eaGroupPauseExtraPars, nullptr);
			if (m_postInfo->GetNzbInfo()->GetRemainingSize() > 0)
			{
				m_postInfo->GetNzbInfo()->SetParRenameStatus(NzbInfo::rsNone);
			}
		}
#endif
	}
	else if (m_kind == jkRar)
	{
		m_postInfo->GetNzbInfo()->SetRarRenameStatus((NzbInfo::EPostRenameStatus)initialStatus);
	}
#ifndef DISABLE_PARCHECK
	if (reqParCheck)
	{
		m_parRenamer.PrintMessage(Message::mkInfo, "Requesting par-check/repair for %s to restore missing files ", m_parRenamer.GetInfoName());
		m_postInfo->SetRequestParCheck(true);
	}
#endif

	m_postInfo->SetWorking(false);
}

#ifndef DISABLE_PARCHECK
void RenameController::UpdateParRenameProgress()
{
	GuardedDownloadQueue guard = DownloadQueue::Guard();

	m_postInfo->SetProgressLabel(m_parRenamer.GetProgressLabel());
	m_postInfo->SetStageProgress(m_parRenamer.GetStageProgress());
}
#endif

void RenameController::UpdateRarRenameProgress()
{
	GuardedDownloadQueue guard = DownloadQueue::Guard();

	m_postInfo->SetProgressLabel(m_rarRenamer.GetProgressLabel());
	m_postInfo->SetStageProgress(m_rarRenamer.GetStageProgress());
}

/**
*  Update file name in the CompletedFiles-list of NZBInfo
*/
void RenameController::RegisterRenamedFile(const char* oldFilename, const char* newFilename)
{
	// under the queue lock: a queue save (another download added, an edit) reads
	// the completed-file names this changes (the message is printed outside it:
	// printing takes the lock too)
	bool found;
	{
		GuardedDownloadQueue guard = DownloadQueue::Guard();
		found = m_postInfo->GetNzbInfo()->RenameCompletedFile(oldFilename, newFilename);
	}
	if (!found)
	{
		PrintMessage(Message::mkWarning,
			"Could not find completed-file entry for %s while renaming to %s",
			oldFilename, newFilename);
	}
	++m_renamedCount;
}
