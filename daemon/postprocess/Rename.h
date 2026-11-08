/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2016 Andrey Prygunkov <hugbug@users.sourceforge.net>
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


#ifndef RENAME_H
#define RENAME_H

#include <exception>
#include <new>
#include <string>
#include "Thread.h"
#include "DownloadInfo.h"
#include "ScriptController.h"
#include "Options.h"
#include "DiskState.h"
#include "Log.h"
#include "FileSystem.h"
#include "nzbget_rs.h"

class RenameController : public Thread, public ScriptController
{
public:
	enum EJobKind
	{
		jkPar,
		jkRar
	};

	RenameController()
		: m_postInfo(nullptr)
		, m_kind(jkPar)
	{
		debug("Creating RenameController");
	}

	virtual void Run()
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

		const char* archiveKindPrefix = nzbget_rs_rename_controller_archive_kind_prefix(
			(int)m_kind, (int)jkRar);
		PrintMessage(Message::mkInfo, "Checking renamed %sfiles for %s",
			archiveKindPrefix, *nzbName);

		ExecRename(destDir, finalDir, nzbName);

		int runOutcome = nzbget_rs_rename_controller_classify_run_outcome(
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

		RenameCompleted(nzbName);
	}

	static void StartJob(PostInfo* postInfo, EJobKind kind)
	{
		RenameController* renameController = new RenameController();
		renameController->m_postInfo = postInfo;
		renameController->m_kind = kind;
		renameController->SetAutoDestroy(false);

		postInfo->SetPostThread(renameController);

		renameController->Start();
	}

protected:
	virtual void AddMessage(Message::EKind kind, const char* text)
	{
		m_postInfo->GetNzbInfo()->AddMessage(kind, text);
	}

private:
	PostInfo* m_postInfo;
	int m_renamedCount = 0;
	bool m_hasMissedFiles = false;
	bool m_hasDamagedParFiles = false;
	EJobKind m_kind;

	void UpdateProgress(const char* label, int stage)
	{
		GuardedDownloadQueue guard = DownloadQueue::Guard();

		m_postInfo->SetProgressLabel(label ? label : "");
		m_postInfo->SetStageProgress(stage);
	}

	void ExecRename(const char* destDir, const char* finalDir, const char* nzbName)
	{
		if (m_kind == jkPar)
		{
#ifndef DISABLE_PARCHECK
			int detectMissingInt = 0;
			bool useFinal = nzbget_rs_rename_controller_eval_par_config(
				(int)m_postInfo->GetNzbInfo()->GetUnpackStatus(),
				(int)NzbInfo::usSuccess,
				(int)NzbInfo::usNone,
				Util::EmptyStr(finalDir) ? 1 : 0,
				&detectMissingInt) != 0;
			int renamedCount = 0;
			int hasMissedFiles = 0;
			int hasDamagedParFiles = 0;
			nzbget_rs_par_renamer_execute(
				nzbName ? nzbName : "",
				(useFinal ? finalDir : destDir) ? (useFinal ? finalDir : destDir) : "",
				g_Options ? g_Options->GetParIgnoreExt() : "",
				detectMissingInt,
				PATH_SEPARATOR,
				this,
				+[](void* rawCtx) noexcept -> int
				{
					return static_cast<RenameController*>(rawCtx)->IsStopped() ? 1 : 0;
				},
				+[](void* rawCtx, const char* label, int stage) noexcept
				{
					static_cast<RenameController*>(rawCtx)->UpdateProgress(label, stage);
				},
				+[](void* rawCtx, int kind, const char* text) noexcept
				{
					auto* self = static_cast<RenameController*>(rawCtx);
					Message::EKind msgKind = kind == 0 ? Message::mkInfo : kind == 1 ? Message::mkWarning : Message::mkError;
					self->PrintMessage(msgKind, "%s", text ? text : "");
				},
				+[](void* rawCtx, const char* name) noexcept
				{
					static_cast<RenameController*>(rawCtx)->m_postInfo->GetParredFiles()->push_back(name ? name : "");
				},
				+[](void* rawCtx, const char* oldRel, const char* newRel) noexcept
				{
					static_cast<RenameController*>(rawCtx)->RegisterRenamedFile(
						oldRel ? oldRel : "", newRel ? newRel : "");
				},
				&renamedCount,
				&hasMissedFiles,
				&hasDamagedParFiles);
			m_hasMissedFiles = hasMissedFiles != 0;
			m_hasDamagedParFiles = hasDamagedParFiles != 0;
#endif
		}
		else if (m_kind == jkRar)
		{
			const char* password = "";
			NzbParameter* parameter = m_postInfo->GetNzbInfo()->GetParameters()->Find("*Unpack:Password");
			if (parameter)
			{
				password = parameter->GetValue();
			}

			struct Context
			{
				RenameController* self;
				std::exception_ptr error;

				int Fail()
				{
					error = std::current_exception();
					return -1;
				}
			} context{this, nullptr};

			NzbgetRsRarRenamerRunEnv env{};
			env.ctx = &context;
			env.stopped = +[](void* ctx) noexcept -> int
			{
				Context& c = *static_cast<Context*>(ctx);
				if (c.error) return -1;
				try { return c.self->IsStopped() ? 1 : 0; } catch (...) { return c.Fail(); }
			};
			env.progress = +[](void* ctx, const char* label, int stage) noexcept -> int
			{
				Context& c = *static_cast<Context*>(ctx);
				if (c.error) return -1;
				try { c.self->UpdateProgress(label, stage); return 0; } catch (...) { return c.Fail(); }
			};
			env.message = +[](void* ctx, int kind, const char* text) noexcept -> int
			{
				Context& c = *static_cast<Context*>(ctx);
				if (c.error) return -1;
				try
				{
					c.self->PrintMessage(
						kind == 0 ? Message::mkInfo : kind == 1 ? Message::mkWarning : Message::mkError,
						"%s", text ? text : "");
					return 0;
				}
				catch (...) { return c.Fail(); }
			};
			env.registerRenamed = +[](void* ctx, const char* oldBase, const char* newBase) noexcept -> int
			{
				Context& c = *static_cast<Context*>(ctx);
				if (c.error) return -1;
				try { c.self->RegisterRenamedFile(oldBase, newBase); return 0; } catch (...) { return c.Fail(); }
			};

#ifndef DISABLE_TLS
			const int tlsEnabled = 1;
#else
			const int tlsEnabled = 0;
#endif
			int renamedCount = 0;
			nzbget_rs_rar_renamer_execute(
				&env, destDir ? destDir : "", nzbName ? nzbName : "", password,
				g_Options ? g_Options->GetUnpackIgnoreExt() : "", tlsEnabled, &renamedCount);
			if (context.error)
			{
				std::rethrow_exception(context.error);
			}
		}
	}

	void RenameCompleted(const char* nzbName)
	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();

		int reqExtraPars = 0;
		int reqParCheck = 0;
#ifndef DISABLE_PARCHECK
		int hasDamagedParFiles = m_hasDamagedParFiles ? 1 : 0;
		int hasMissedFiles = m_hasMissedFiles ? 1 : 0;
#else
		int hasDamagedParFiles = 0;
		int hasMissedFiles = 0;
#endif
		int initialStatus = nzbget_rs_rename_controller_complete_action(
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
				PrintMessage(Message::mkInfo, "Requesting extra par2-files for %s to perform par-rename", nzbName ? nzbName : "");
				downloadQueue->EditEntry(m_postInfo->GetNzbInfo()->GetId(), DownloadQueue::eaGroupResume, nullptr);
				downloadQueue->EditEntry(m_postInfo->GetNzbInfo()->GetId(), DownloadQueue::eaGroupPauseExtraPars, nullptr);
				if (nzbget_rs_rename_controller_should_reset_par_status(
					m_postInfo->GetNzbInfo()->GetRemainingSize()) != 0)
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
			PrintMessage(Message::mkInfo, "Requesting par-check/repair for %s to restore missing files ", nzbName ? nzbName : "");
			m_postInfo->SetRequestParCheck(true);
		}
#endif

		m_postInfo->SetWorking(false);
	}

	void RegisterRenamedFile(const char* oldFilename, const char* newFilename)
	{
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
};

class PostRenamerController final : public Thread, public ScriptController
{
public:
	static void StartJob(PostInfo* postInfo, bool postUnpack)
	{
		PostRenamerController* controller = new (std::nothrow) PostRenamerController();
		if (!controller)
		{
			error("Failed to allocate memory for PostRenamerController");
			return;
		}
		controller->m_postInfo = postInfo;
		controller->m_postUnpack = postUnpack;
		controller->SetAutoDestroy(false);
		postInfo->SetPostThread(controller);
		controller->Start();
	}

	void Run() override
	{
		std::string metaName;
		std::string nzbName;
		std::string dstDir;
		{
			GuardedDownloadQueue guard = DownloadQueue::Guard();
			NzbInfo* nzbInfo = m_postInfo->GetNzbInfo();
			metaName = nzbInfo->GetMetaName();
			nzbName = nzbInfo->GetName() ? nzbInfo->GetName() : "";
			dstDir = nzbInfo->GetDestDir() ? nzbInfo->GetDestDir() : "";
		}

		int status = nzbget_rs_post_renamer_run(
			m_postUnpack ? 1 : 0,
			metaName.c_str(),
			nzbName.c_str(),
			dstDir.c_str(),
			g_Options->GetRenameIgnoreExt(),
			this,
			+[](void* rawCtx, const char* infoName) {
				static_cast<PostRenamerController*>(rawCtx)->SetInfoName(infoName ? infoName : "");
			},
			+[](void* rawCtx) -> int {
				return static_cast<PostRenamerController*>(rawCtx)->IsStopped() ? 1 : 0;
			},
			+[](void* rawCtx, int kind, const char* msg) {
				auto* self = static_cast<PostRenamerController*>(rawCtx);
				Message::EKind msgKind = kind == 1
					? Message::mkWarning
					: kind == 2
						? Message::mkError
						: Message::mkInfo;
				self->PrintMessage(msgKind, "%s", msg ? msg : "");
			},
			+[](void* rawCtx, const char* oldName, const char* newName) {
				auto* self = static_cast<PostRenamerController*>(rawCtx);
				GuardedDownloadQueue guard = DownloadQueue::Guard();
				if (self->m_postInfo && self->m_postInfo->GetNzbInfo())
				{
					self->m_postInfo->GetNzbInfo()->RenameCompletedFile(
						oldName ? oldName : "", newName ? newName : "");
				}
			});

		GuardedDownloadQueue guard = DownloadQueue::Guard();
		if (m_postUnpack)
		{
			m_postInfo->GetNzbInfo()->SetPostUnpackRenamingStatus(
				static_cast<NzbInfo::PostUnpackRenamingStatus>(status));
		}
		else
		{
			m_postInfo->GetNzbInfo()->SetPostDownloadRenamingStatus(
				static_cast<NzbInfo::PostDownloadRenamingStatus>(status));
		}
		m_postInfo->SetWorking(false);
	}

protected:
	void AddMessage(Message::EKind kind, const char* text) override
	{
		m_postInfo->GetNzbInfo()->AddMessage(kind, text);
	}

private:
	PostInfo* m_postInfo = nullptr;
	bool m_postUnpack = false;
};

#endif
