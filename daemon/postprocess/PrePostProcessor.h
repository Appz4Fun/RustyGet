/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2007-2016 Andrey Prygunkov <hugbug@users.sourceforge.net>
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


#ifndef PREPOSTPROCESSOR_H
#define PREPOSTPROCESSOR_H
#include "DownloadInfo.h"

class PrePostProcessor : public Thread, public Observer
{
public:
	PrePostProcessor();
	void Run() override;
	void Stop() override;
	bool HasMoreJobs() { return m_queuedJobs > 0; }
	int GetJobCount() { return m_queuedJobs; }
	bool HasActiveUnpack(bool includeWaitingVolume = true);
	bool HasActiveUnpack(DownloadQueue* downloadQueue, bool includeWaitingVolume = true);
	bool EditList(DownloadQueue* downloadQueue, IdList* idList, DownloadQueue::EEditAction action,
		const char* args);
	void NzbAdded(DownloadQueue* downloadQueue, NzbInfo* nzbInfo);
	void NzbDownloaded(DownloadQueue* downloadQueue, NzbInfo* nzbInfo);
	// A late rename or a draining paused PAR request can make the collection
	// ready without completing another file. Requires the download-queue lock.
	void CheckDownloadCompleted(DownloadQueue* downloadQueue, NzbInfo* nzbInfo);

protected:
	void Update(Subject*, void* aspect) override { DownloadQueueUpdate(aspect); }

private:
	int m_queuedJobs = 0;
	RawNzbList m_activeJobs;
	std::recursive_mutex m_activeJobsMutex;
	std::mutex m_waitMutex;
	std::condition_variable m_waitCond;

	void CheckPostQueue();
	void CheckRequestPar(DownloadQueue* downloadQueue);
	void CleanupJobs(DownloadQueue* downloadQueue);
	bool CanRunMoreJobs(bool* allowPar);
	NzbInfo* PickNextJob(DownloadQueue* downloadQueue, bool allowPar);
	void StartJob(DownloadQueue* downloadQueue, PostInfo* postInfo, bool allowPar);
	void EnterStage(DownloadQueue* downloadQueue, PostInfo* postInfo, PostInfo::EStage stage);
	void SanitisePostQueue();
	void UpdatePauseState();
	void NzbFound(DownloadQueue* downloadQueue, NzbInfo* nzbInfo);
	void NzbDeleted(DownloadQueue* downloadQueue, NzbInfo* nzbInfo);
	void NzbCompleted(DownloadQueue* downloadQueue, NzbInfo* nzbInfo, bool saveQueue);
	void JobCompleted(DownloadQueue* downloadQueue, PostInfo* postInfo);
	bool PostQueueDelete(DownloadQueue* downloadQueue, IdList* idList);
	void DownloadQueueUpdate(void* aspect);
	void DeleteCleanup(NzbInfo* nzbInfo);
	void WaitJobs();
	void FileDownloaded(DownloadQueue* downloadQueue, NzbInfo* nzbInfo, FileInfo* fileInfo);
};

extern PrePostProcessor* g_PrePostProcessor;

#include "Options.h"
#include "Log.h"
#include "HistoryCoordinator.h"
#include "DupeCoordinator.h"
#include "ExtensionManager.h"
#include "Util.h"
#include "FileSystem.h"
#include "UnpackController.h"
#include "Repair.h"
#include "StreamRepair.h"
#include "DiskState.h"
#include "QueueScript.h"
#include "DirectUnpack.h"
#include "ParDamage.h"
#include <exception>
#include <new>
#include <mutex>
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

class PostScriptController : public Thread, public NzbScriptController
{
public:
	virtual void Run()
	{
		StringBuilder scriptCommaList;

		{
			GuardedDownloadQueue guard = DownloadQueue::Guard();
			size_t pcount = m_postInfo->GetNzbInfo()->GetParameters()->size();
			std::vector<const char*> pnames;
			std::vector<const char*> pvals;
			pnames.reserve(pcount);
			pvals.reserve(pcount);
			for (NzbParameter& parameter : m_postInfo->GetNzbInfo()->GetParameters())
			{
				pnames.push_back(parameter.GetName());
				pvals.push_back(parameter.GetValue());
			}
			NzbgetRsBuf listBuf = nzbget_rs_sus_build_enabled_script_comma_list(pnames.data(), pvals.data(), pcount);
			if (listBuf.data)
			{
				scriptCommaList.Append(listBuf.data);
			}
			nzbget_rs_free(listBuf);
			m_postInfo->GetNzbInfo()->GetScriptStatuses()->clear();
		}

		ExecuteScriptList(scriptCommaList);

		m_postInfo->SetStage(PostInfo::ptFinished);
		m_postInfo->SetWorking(false);
	}

	virtual void Stop()
	{
		debug("Stopping post-process-script");
		Thread::Stop();
		Terminate();
	}

	static void StartJob(PostInfo* postInfo)
	{
		if (!postInfo)
		{
			return;
		}

		if (postInfo->GetNzbInfo() && postInfo->GetNzbInfo()->GetSkipScriptProcessing())
		{
			return;
		}

		PostScriptController* scriptController = new PostScriptController();
		scriptController->m_postInfo = postInfo;
		scriptController->SetAutoDestroy(false);
		scriptController->m_prefixLen = 0;

		postInfo->SetPostThread(scriptController);

		scriptController->Start();
	}

protected:
	virtual void ExecuteScript(std::shared_ptr<const Extension::Script> script)
	{
		// if any script has requested par-check, do not execute other scripts
		if (!script->GetPostScript() || m_postInfo->GetRequestParCheck())
		{
			return;
		}

		m_script = std::move(script);

		PrintMessage(Message::mkInfo, "Executing post-process-script %s for %s", m_script->GetName(), m_postInfo->GetNzbInfo()->GetName());

		BString<1024> progressLabel("Executing post-process-script %s", m_script->GetName());
		{
			GuardedDownloadQueue guard = DownloadQueue::Guard();
			m_postInfo->SetProgressLabel(progressLabel);
		}

		SetArgs({m_script->GetEntry()});

		BString<1024> infoName("post-process-script %s for %s", m_script->GetName(), m_postInfo->GetNzbInfo()->GetName());
		SetInfoName(infoName);

		SetLogPrefix(m_script->GetDisplayName());
		m_prefixLen = static_cast<int>(strlen(m_script->GetDisplayName())) + 2; // 2 = strlen(": ");
		PrepareParams(m_script->GetName());

		int exitCode = Execute();

		infoName[0] = 'P';

		SetLogPrefix(nullptr);
		ScriptStatus::EStatus status = AnalyseExitCode(exitCode, infoName);

		{
			GuardedDownloadQueue guard = DownloadQueue::Guard();
			m_postInfo->GetNzbInfo()->GetScriptStatuses()->emplace_back(m_script->GetName(), status);
		}
	}

	virtual void AddMessage(Message::EKind kind, const char* text)
	{
		const char* msgText = text + m_prefixLen;

		NzbgetRsPostScriptCmd cmd{};
		int cmdKind = nzbget_rs_sus_parse_post_script_cmd(msgText, &cmd);
		if (cmdKind != 0)
		{
			debug("Command %s detected", msgText + 6);
			if (cmdKind == 1)
			{
				GuardedDownloadQueue guard = DownloadQueue::Guard();
				m_postInfo->GetNzbInfo()->SetFinalDir(msgText + cmd.val_offset);
			}
			else if (cmdKind == 2)
			{
				GuardedDownloadQueue guard = DownloadQueue::Guard();
				m_postInfo->GetNzbInfo()->SetDestDir(msgText + cmd.val_offset);
			}
			else if (cmdKind == 3)
			{
				std::string param(msgText + cmd.key_offset, cmd.key_len);
				GuardedDownloadQueue guard = DownloadQueue::Guard();
				m_postInfo->GetNzbInfo()->GetParameters()->SetParameter(param.c_str(), msgText + cmd.val_offset);
			}
			else if (cmdKind == 4)
			{
				SetLogPrefix(nullptr);
				PrintMessage(Message::mkWarning, "Marking %s as bad", m_postInfo->GetNzbInfo()->GetName());
				SetLogPrefix(m_script->GetDisplayName());
				m_postInfo->GetNzbInfo()->SetMarkStatus(NzbInfo::ksBad);
			}
			else
			{
				m_postInfo->GetNzbInfo()->PrintMessage(Message::mkError,
					"Invalid command \"%s\" received from %s", msgText, GetInfoName());
			}
		}
		else
		{
			m_postInfo->GetNzbInfo()->AddMessage(kind, text);
			GuardedDownloadQueue guard = DownloadQueue::Guard();
			m_postInfo->SetProgressLabel(text);
		}

		if (g_WorkState->GetPausePostProcess() && !m_postInfo->GetNzbInfo()->GetForcePriority())
		{
			time_t stageTime = m_postInfo->GetStageTime();
			time_t startTime = m_postInfo->GetStartTime();
			time_t waitTime = Util::CurrentTime();

			while (g_WorkState->GetPausePostProcess() && !m_postInfo->GetNzbInfo()->GetForcePriority() && !IsStopped())
			{
				Util::Sleep(100);

				time_t delta = Util::CurrentTime() - waitTime;
				if (stageTime > 0)
				{
					m_postInfo->SetStageTime(stageTime + delta);
				}
				if (startTime > 0)
				{
					m_postInfo->SetStartTime(startTime + delta);
				}
			}
		}
	}

private:
	PostInfo* m_postInfo;
	int m_prefixLen;
	std::shared_ptr<const Extension::Script> m_script;

	void PrepareParams(const char* scriptName)
	{
		GuardedDownloadQueue guard = DownloadQueue::Guard();

		ResetEnv();

		NzbInfo* nzbInfo = m_postInfo->GetNzbInfo();
		BString<1024> status = nzbInfo->MakeTextStatus(true);
		std::vector<int> serverIds;
		std::vector<int> serverSuccess;
		std::vector<int> serverFailed;
		serverIds.reserve(nzbInfo->GetServerStats()->size());
		serverSuccess.reserve(nzbInfo->GetServerStats()->size());
		serverFailed.reserve(nzbInfo->GetServerStats()->size());
		for (ServerStat& serverStat : nzbInfo->GetServerStats())
		{
			serverIds.push_back(serverStat.GetServerId());
			serverSuccess.push_back(serverStat.GetSuccessArticles());
			serverFailed.push_back(serverStat.GetFailedArticles());
		}

		NzbgetRsPostScriptEnv env{};
		env.nzb_id = nzbInfo->GetId();
		env.nzb_name = nzbInfo->GetName();
		env.dest_dir = nzbInfo->GetDestDir();
		env.filename = nzbInfo->GetFilename();
		env.queued_filename = nzbInfo->GetQueuedFilename();
		env.url = nzbInfo->GetUrl();
		env.final_dir = nzbInfo->GetFinalDir();
		env.category = nzbInfo->GetCategory();
		env.health = nzbInfo->CalcHealth();
		env.critical_health = nzbInfo->CalcCriticalHealth(false);
		env.dupe_key = nzbInfo->GetDupeKey();
		env.dupe_score = nzbInfo->GetDupeScore();
		env.dupe_mode = static_cast<int>(nzbInfo->GetDupeMode());
		env.status = status;
		env.script_status = static_cast<int>(nzbInfo->GetScriptStatuses()->CalcTotalStatus());
		env.par_status = static_cast<int>(nzbInfo->GetParStatus());
		env.unpack_status = static_cast<int>(nzbInfo->GetUnpackStatus());
		env.delete_status = static_cast<int>(nzbInfo->GetDeleteStatus());
		env.mark_status = static_cast<int>(nzbInfo->GetMarkStatus());
		env.total_articles = nzbInfo->GetTotalArticles();
		env.success_articles = nzbInfo->GetSuccessArticles();
		env.failed_articles = nzbInfo->GetFailedArticles();
		env.server_ids = serverIds.data();
		env.server_success_articles = serverSuccess.data();
		env.server_failed_articles = serverFailed.data();
		env.server_count = serverIds.size();

		nzbget_rs_post_script_prepare_env(
			&env,
			this,
			[](void* ctx, const char* name, const char* value)
			{
				static_cast<PostScriptController*>(ctx)->SetEnvVar(name, value);
			},
			[](void* ctx, const char* name, int value)
			{
				static_cast<PostScriptController*>(ctx)->SetIntEnvVar(name, value);
			});

		PrepareEnvScript(nzbInfo->GetParameters(), scriptName);
	}

	ScriptStatus::EStatus AnalyseExitCode(int exitCode, const char* upInfoName)
	{
		// The ScriptStatus is accumulated for all scripts:
		// If any script has failed the status is "failure", etc.

#ifndef DISABLE_PARCHECK
		constexpr int parcheckEnabled = 1;
#else
		constexpr int parcheckEnabled = 0;
#endif
		int msgAction = 0;
		int reqParCheck = 0;
		int status = nzbget_rs_sus_analyse_post_script_exit_code(
			exitCode,
			static_cast<int>(m_postInfo->GetNzbInfo()->GetParStatus()),
			static_cast<int>(NzbInfo::psSkipped),
			parcheckEnabled,
			static_cast<int>(ScriptStatus::srNone),
			static_cast<int>(ScriptStatus::srFailure),
			static_cast<int>(ScriptStatus::srSuccess),
			&msgAction,
			&reqParCheck);
		switch (msgAction)
		{
			case 1:
				PrintMessage(Message::mkInfo, "%s successful", upInfoName);
				break;
			case 2:
				PrintMessage(Message::mkError, "%s failed", upInfoName);
				break;
			case 3:
				PrintMessage(Message::mkInfo, "%s skipped", upInfoName);
				break;
			case 4:
				PrintMessage(Message::mkError, "%s requested par-check/repair, but the collection was already checked", upInfoName);
				break;
			case 5:
				PrintMessage(Message::mkInfo, "%s requested par-check/repair", upInfoName);
				break;
			default:
				PrintMessage(Message::mkError, "%s failed (terminated with unknown status)", upInfoName);
				break;
		}
		if (reqParCheck)
		{
			m_postInfo->SetRequestParCheck(true);
			m_postInfo->SetForceRepair(true);
		}
		return static_cast<ScriptStatus::EStatus>(status);
	}
};

inline PrePostProcessor::PrePostProcessor()
{
	debug("Creating PrePostProcessor");

	DownloadQueue::Guard()->Attach(this);
}

inline void PrePostProcessor::Run()
{
	debug("Entering PrePostProcessor-loop");

	while (!DownloadQueue::IsLoaded())
	{
		Util::Sleep(20);
	}

	if (g_Options->GetServerMode())
	{
		SanitisePostQueue();
	}

	while (!IsStopped())
	{
		if (g_WorkState->GetTempPausePostprocess())
		{
			// Postprocess is paused: just wait and loop
			Util::Sleep(200);
			continue;
		}

		if (m_queuedJobs)
		{
			// check post-queue every 200 msec
			CheckPostQueue();
			Util::Sleep(200);
		}
		else
		{
			// Wait until we get the stop signal or more jobs in the queue
			std::unique_lock<std::mutex> lk(m_waitMutex);
			m_waitCond.wait(lk, [&]{ return m_queuedJobs || IsStopped(); });
		}
	}

	WaitJobs();

	debug("Exiting PrePostProcessor-loop");
}

inline void PrePostProcessor::WaitJobs()
{
	debug("PrePostProcessor: waiting for jobs to complete");

	// wait 5 seconds until all post-processing jobs gracefully finish
	time_t waitStart = Util::CurrentTime();
	while (Util::CurrentTime() - waitStart < 5)
	{
		{
			GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
			std::lock_guard<std::recursive_mutex> guard(m_activeJobsMutex);
			if (m_activeJobs.empty())
			{
				break;
			}
		}
		CheckPostQueue();
		Util::Sleep(200);
	}

	// kill remaining post-processing jobs; not safe but we can't wait any longer
	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
		std::lock_guard<std::recursive_mutex> guard(m_activeJobsMutex);
		for (NzbInfo* postJob : m_activeJobs)
		{
			if (postJob->GetPostInfo() && postJob->GetPostInfo()->GetPostThread())
			{
				Thread* thread = postJob->GetPostInfo()->GetPostThread();
				postJob->GetPostInfo()->SetPostThread(nullptr);
				warn("Terminating active post-process job for %s", postJob->GetName());
				thread->Kill();
				delete thread;
			}
		}
	}

	// wait 5 seconds until direct unpack and live repair threads gracefully finish
	waitStart = Util::CurrentTime();
	while (Util::CurrentTime() - waitStart < 5)
	{
		{
			GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
			if (std::find_if(downloadQueue->GetQueue()->begin(),
				downloadQueue->GetQueue()->end(),
				[](const std::unique_ptr<NzbInfo>& nzbInfo)
				{
					return nzbInfo->GetUnpackThread() != nullptr ||
						nzbInfo->GetLiveRepairThread() != nullptr;
				}) == downloadQueue->GetQueue()->end())
			{
				break;
			}
		}
		Util::Sleep(200);
	}

	// disconnect remaining direct unpack and live repair jobs
	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
		for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
		{
			nzbInfo->SetUnpackThread(nullptr);
			nzbInfo->SetLiveRepairThread(nullptr);
		}
	}

	debug("PrePostProcessor: Jobs are completed");
}

inline void PrePostProcessor::Stop()
{
	Thread::Stop();

	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
		std::lock_guard<std::recursive_mutex> guard(m_activeJobsMutex);

		for (NzbInfo* postJob : m_activeJobs)
		{
			if (postJob->GetPostInfo() && postJob->GetPostInfo()->GetPostThread())
			{
				postJob->GetPostInfo()->GetPostThread()->Stop();
			}
		}

		for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
		{
			if (nzbInfo->GetUnpackThread())
			{
				((DirectUnpack*)nzbInfo->GetUnpackThread())->Stop(downloadQueue, nzbInfo);
			}
			if (nzbInfo->GetLiveRepairThread())
			{
				nzbInfo->GetLiveRepairThread()->Stop();
			}
		}
	}

	// Resume Run() to exit it
	std::lock_guard<std::mutex> guard(m_waitMutex);
	m_waitCond.notify_all();
}

inline bool PrePostProcessor::HasActiveUnpack(bool includeWaitingVolume)
{
	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
	return HasActiveUnpack(downloadQueue, includeWaitingVolume);
}

inline bool PrePostProcessor::HasActiveUnpack(DownloadQueue* downloadQueue, bool includeWaitingVolume)
{
	// Check post-processing active jobs for UnpackController
	{
		std::lock_guard<std::recursive_mutex> guard(m_activeJobsMutex);
		for (NzbInfo* postJob : m_activeJobs)
		{
			if (postJob->GetPostInfo() && postJob->GetPostInfo()->GetStage() == PostInfo::ptUnpacking)
			{
				return true;
			}
		}
	}

	// Check direct unpack jobs: does a direct unpack own the unpack slot / actively extract?
	for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
	{
		if (nzbInfo->GetUnpackThread())
		{
			DirectUnpack* directUnpack = (DirectUnpack*)nzbInfo->GetUnpackThread();
			if (includeWaitingVolume ||
				(directUnpack->IsUnpacking() && !directUnpack->IsWaitingVolume()))
			{
				return true;
			}
		}
	}

	return false;
}

/**
 * Reset the state of items after reloading from disk and
 * delete items which could not be resumed.
 * Also count the number of post-jobs.
 */
inline void PrePostProcessor::SanitisePostQueue()
{
	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
	for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
	{
		if (nzbInfo->GetDirectUnpackStatus() == NzbInfo::nsRunning && !nzbInfo->GetUnpackThread())
		{
			nzbInfo->SetDirectUnpackStatus(NzbInfo::nsNone);
		}
		PostInfo* postInfo = nzbInfo->GetPostInfo();
		if (!postInfo && nzbInfo->GetKind() == NzbInfo::nkNzb &&
			!nzbInfo->GetCompletedFiles()->empty() &&
			nzbInfo->GetDeleteStatus() == NzbInfo::dsNone &&
			nzbInfo->IsDownloadCompleted(true))
		{
			// its last file finished while the program stopped (a reload, a
			// shutdown): the event that queues it for post-processing was
			// dropped then, and only paused par2-files are left, so nothing
			// else would ever start it
			nzbInfo->PrintMessage(Message::mkInfo,
				"Collection %s completely downloaded", nzbInfo->GetName());
			g_QueueScriptCoordinator->EnqueueScript(nzbInfo, QueueScriptCoordinator::qeNzbDownloaded);
			NzbDownloaded(downloadQueue, nzbInfo);
			postInfo = nzbInfo->GetPostInfo();
			if (postInfo)
			{
				m_queuedJobs--;	// counted again below
			}
		}
		if (postInfo)
		{
			m_queuedJobs++;
			int nextStage = 0;
			int reqParCheck = 0;
			nzbget_rs_prepost_sanitise_post_stage(
				(int)postInfo->GetStage(),
				(int)PostInfo::ptExecutingScript,
				(int)PostInfo::ptFinished,
				(int)PostInfo::ptQueued,
				FileSystem::DirectoryExists(nzbInfo->GetDestDir()) ? 1 : 0,
				nzbInfo->GetUnpackTried() ? 1 : 0,
				(int)nzbInfo->GetParStatus(),
				(int)NzbInfo::psSkipped,
				&nextStage,
				&reqParCheck);
			postInfo->SetStage((PostInfo::EStage)nextStage);
			if (reqParCheck)
			{
				postInfo->SetRequestParCheck(true);
			}
			postInfo->SetWorking(false);
		}
	}
}

inline void PrePostProcessor::DownloadQueueUpdate(void* aspect)
{
	if (IsStopped())
	{
		return;
	}

	DownloadQueue::Aspect* queueAspect = (DownloadQueue::Aspect*)aspect;
	if (queueAspect->action == DownloadQueue::eaNzbFound ||
		queueAspect->action == DownloadQueue::eaUrlFound)
	{
		NzbFound(queueAspect->downloadQueue, queueAspect->nzbInfo);
	}
	else if (queueAspect->action == DownloadQueue::eaNzbAdded ||
		queueAspect->action == DownloadQueue::eaUrlAdded)
	{
		NzbAdded(queueAspect->downloadQueue, queueAspect->nzbInfo);
	}
	else if (queueAspect->action == DownloadQueue::eaNzbNamed)
	{
		g_QueueScriptCoordinator->EnqueueScript(queueAspect->nzbInfo, QueueScriptCoordinator::qeNzbNamed);
		CheckDownloadCompleted(queueAspect->downloadQueue, queueAspect->nzbInfo);
	}
	else if (queueAspect->action == DownloadQueue::eaNzbDeleted &&
		queueAspect->nzbInfo->GetDeleting() &&
		!queueAspect->nzbInfo->GetPostInfo() &&
		queueAspect->nzbInfo->GetFileList()->empty())
	{
		// the deleting of nzbs is usually handled via eaFileDeleted-event, but when deleting nzb without
		// any files left the eaFileDeleted-event is not fired and we need to process eaNzbDeleted-event instead
		queueAspect->nzbInfo->PrintMessage(Message::mkInfo,
			"Collection %s deleted from queue", queueAspect->nzbInfo->GetName());
		NzbDeleted(queueAspect->downloadQueue, queueAspect->nzbInfo);
	}
	else if (queueAspect->action == DownloadQueue::eaUrlDeleted)
	{
		NzbDeleted(queueAspect->downloadQueue, queueAspect->nzbInfo);
	}
	else if (queueAspect->action == DownloadQueue::eaUrlFailed)
	{
		NzbCompleted(queueAspect->downloadQueue, queueAspect->nzbInfo, true);
	}
	else if (queueAspect->action == DownloadQueue::eaFileCompleted ||
		queueAspect->action == DownloadQueue::eaFileDeleted)
	{
		if (queueAspect->action == DownloadQueue::eaFileCompleted)
		{
			FileDownloaded(queueAspect->downloadQueue, queueAspect->nzbInfo, queueAspect->fileInfo);
		}

#ifndef DISABLE_PARCHECK
		{
			std::lock_guard<std::recursive_mutex> guard(m_activeJobsMutex);
			for (NzbInfo* postJob : m_activeJobs)
			{
				PostInfo* pjInfo = postJob ? postJob->GetPostInfo() : nullptr;
				if (postJob &&
					queueAspect->fileInfo->GetNzbInfo() == postJob &&
					pjInfo &&
					pjInfo->GetPostThread() &&
					pjInfo->GetStage() >= PostInfo::ptLoadingPars &&
					pjInfo->GetStage() <= PostInfo::ptVerifyingRepaired &&
					((RepairController*)pjInfo->GetPostThread())->AddPar(
						queueAspect->fileInfo, queueAspect->action == DownloadQueue::eaFileDeleted))
				{
					return;
				}
			}
		}
#endif

		int eventAction = nzbget_rs_prepost_classify_file_event(
			queueAspect->action == DownloadQueue::eaFileCompleted ? 1 : 0,
			queueAspect->action == DownloadQueue::eaFileDeleted ? 1 : 0,
			queueAspect->fileInfo->GetDupeDeleted() ? 1 : 0,
			(int)queueAspect->fileInfo->GetNzbInfo()->GetDeleteStatus(),
			(int)NzbInfo::dsNone,
			(int)NzbInfo::dsHealth,
			queueAspect->nzbInfo->GetPostInfo() ? 1 : 0,
			queueAspect->nzbInfo->IsDownloadCompleted(true) ? 1 : 0,
			queueAspect->nzbInfo->IsDownloadCompleted(false) ? 1 : 0);
		if (eventAction == 1)
		{
			queueAspect->nzbInfo->PrintMessage(Message::mkInfo,
				"Collection %s completely downloaded", queueAspect->nzbInfo->GetName());
			g_QueueScriptCoordinator->EnqueueScript(queueAspect->nzbInfo, QueueScriptCoordinator::qeNzbDownloaded);
			NzbDownloaded(queueAspect->downloadQueue, queueAspect->nzbInfo);
		}
		else if (eventAction == 2)
		{
			queueAspect->nzbInfo->PrintMessage(Message::mkInfo,
				"Collection %s deleted from queue", queueAspect->nzbInfo->GetName());
			NzbDeleted(queueAspect->downloadQueue, queueAspect->nzbInfo);
		}
	}
}

inline void PrePostProcessor::NzbFound(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
{
	if (g_Options->GetDupeCheck() && nzbInfo->GetDupeMode() != dmForce)
	{
		g_DupeCoordinator->NzbFound(downloadQueue, nzbInfo);
	}
}

inline void PrePostProcessor::NzbAdded(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
{
	if (g_Options->GetParCheck() != Options::pcForce)
	{
		downloadQueue->EditEntry(nzbInfo->GetId(), DownloadQueue::eaGroupPauseExtraPars, nullptr);
	}

	if (g_Options->GetReorderFiles() && nzbInfo->GetDeleteStatus() == NzbInfo::dsNone)
	{
		nzbInfo->PrintMessage(Message::mkInfo, "Reordering files for %s", nzbInfo->GetName());
		downloadQueue->EditEntry(nzbInfo->GetId(), DownloadQueue::eaGroupSortFiles, nullptr);
	}

	if (nzbInfo->GetDeleteStatus() == NzbInfo::dsDupe ||
		nzbInfo->GetDeleteStatus() == NzbInfo::dsCopy ||
		nzbInfo->GetDeleteStatus() == NzbInfo::dsGood ||
		nzbInfo->GetDeleteStatus() == NzbInfo::dsScan)
	{
		NzbCompleted(downloadQueue, nzbInfo, false);
	}
	else
	{
		g_QueueScriptCoordinator->EnqueueScript(nzbInfo, QueueScriptCoordinator::qeNzbAdded);
	}
}

inline void PrePostProcessor::CheckDownloadCompleted(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
{
	// Direct rename can pause the last PAR volumes after the data files
	// finished. Their in-flight articles may still be draining; the last
	// such request does not complete a file, so check both transitions.
	if (nzbget_rs_prepost_should_complete_after_direct_rename(
		IsStopped() ? 1 : 0,
		(int)nzbInfo->GetDirectRenameStatus(),
		(int)NzbInfo::tsSuccess,
		nzbInfo->GetPostInfo() ? 1 : 0,
		(int)nzbInfo->GetDeleteStatus(),
		(int)NzbInfo::dsNone,
		!nzbInfo->GetCompletedFiles()->empty() ? 1 : 0,
		nzbInfo->IsDownloadCompleted(true) ? 1 : 0,
		g_Options->GetRawArticle() ? 1 : 0,
		g_Options->GetSkipWrite() ? 1 : 0,
		nzbInfo->GetSkipDiskWrite() ? 1 : 0) != 0)
	{
		nzbInfo->PrintMessage(Message::mkInfo,
			"Collection %s completely downloaded", nzbInfo->GetName());
		g_QueueScriptCoordinator->EnqueueScript(nzbInfo, QueueScriptCoordinator::qeNzbDownloaded);
		NzbDownloaded(downloadQueue, nzbInfo);
	}
}

inline void PrePostProcessor::NzbDownloaded(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
{
	if (!nzbInfo)
	{
		return;
	}

	int enqueueDeleted = 0;
	int setParSkipped = 0;
	int dlRoute = nzbget_rs_prepost_classify_nzb_downloaded_entry(
		nzbInfo->GetSkipScriptProcessing() ? 1 : 0,
		nzbInfo->GetSkipDiskWrite() ? 1 : 0,
		g_Options->GetSkipWrite() ? 1 : 0,
		(int)nzbInfo->GetDeleteStatus(),
		(int)NzbInfo::dsHealth,
		(int)NzbInfo::dsBad,
		nzbInfo->GetPostInfo() != nullptr ? 1 : 0,
		g_Options->GetRawArticle() ? 1 : 0,
		(int)nzbInfo->GetParStatus(),
		(int)NzbInfo::psNone,
		(int)g_Options->GetParCheck(),
		(int)Options::pcAlways,
		(int)Options::pcForce,
		&enqueueDeleted,
		&setParSkipped);
	if (dlRoute == 1)
	{
		if (nzbInfo->GetLiveRepairThread())
		{
			Thread* liveRepairThread = nzbInfo->GetLiveRepairThread();
			nzbInfo->SetLiveRepairThread(nullptr);
			liveRepairThread->Stop();
		}
		nzbInfo->SetCleanupDisk(true);
		DeleteCleanup(nzbInfo);
		NzbCompleted(downloadQueue, nzbInfo, true);
		return;
	}
	if (enqueueDeleted)
	{
		g_QueueScriptCoordinator->EnqueueScript(nzbInfo, QueueScriptCoordinator::qeNzbDeleted);
	}
	if (dlRoute == 2)
	{
		nzbInfo->PrintMessage(Message::mkInfo, "Queueing %s for post-processing", nzbInfo->GetName());

		nzbInfo->EnterPostProcess();

		if (setParSkipped)
		{
			nzbInfo->SetParStatus(NzbInfo::psSkipped);
		}

		if (nzbInfo->GetUnpackThread())
		{
			nzbInfo->GetPostInfo()->SetWorking(true);
			{
				std::lock_guard<std::recursive_mutex> guard(m_activeJobsMutex);
				m_activeJobs.push_back(nzbInfo);
			}
			((DirectUnpack*)nzbInfo->GetUnpackThread())->NzbDownloaded(downloadQueue, nzbInfo);
		}

		if (nzbInfo->GetLiveRepairThread())
		{
			nzbInfo->GetPostInfo()->SetWorking(true);
			if (!nzbInfo->GetUnpackThread())
			{
				std::lock_guard<std::recursive_mutex> guard(m_activeJobsMutex);
				m_activeJobs.push_back(nzbInfo);
			}
		}

		nzbInfo->SetChanged(true);
		downloadQueue->SaveChanged();

		std::lock_guard<std::mutex> guard(m_waitMutex);
		m_queuedJobs++;
		m_waitCond.notify_all();
	}
	else
	{
		if (nzbInfo->GetUnpackThread())
		{
			((DirectUnpack*)nzbInfo->GetUnpackThread())->NzbDownloaded(downloadQueue, nzbInfo);
		}

		if (nzbInfo->GetLiveRepairThread())
		{
			Thread* liveRepairThread = nzbInfo->GetLiveRepairThread();
			nzbInfo->SetLiveRepairThread(nullptr);
			liveRepairThread->Stop();
		}

		NzbCompleted(downloadQueue, nzbInfo, true);
	}
}

inline void PrePostProcessor::NzbDeleted(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
{
	if (nzbInfo->GetUnpackThread())
	{
		((DirectUnpack*)nzbInfo->GetUnpackThread())->NzbDeleted(downloadQueue, nzbInfo);
	}

	if (nzbInfo->GetLiveRepairThread())
	{
		// detach FIRST: the live pass re-checks attachment at every locked
		// touchpoint and aborts without writing anything back
		Thread* liveRepairThread = nzbInfo->GetLiveRepairThread();
		nzbInfo->SetLiveRepairThread(nullptr);
		liveRepairThread->Stop();
	}

	if (nzbInfo->GetDeleteStatus() == NzbInfo::dsNone)
	{
		nzbInfo->SetDeleteStatus(NzbInfo::dsManual);
	}
	bool callDownloaded = nzbInfo->GetDeleteStatus() == NzbInfo::dsHealth ||
		nzbInfo->GetDeleteStatus() == NzbInfo::dsBad;
	nzbInfo->SetDeleting(false);

	DeleteCleanup(nzbInfo);

	if (callDownloaded)
	{
		NzbDownloaded(downloadQueue, nzbInfo);
	}
	else
	{
		NzbCompleted(downloadQueue, nzbInfo, true);
	}
}

inline void PrePostProcessor::NzbCompleted(DownloadQueue* downloadQueue, NzbInfo* nzbInfo, bool saveQueue)
{
	int addToHistoryInt = 0, callDupeCompletedInt = 0, enqueueDeletedScriptInt = 0, needSaveInt = 0;
	nzbget_rs_prepost_eval_nzb_completed(
		(int)nzbInfo->GetDupeHint(),
		(int)NzbInfo::dhRedownloadAuto,
		g_Options->GetKeepHistory(),
		nzbInfo->GetAvoidHistory() ? 1 : 0,
		g_Options->GetDupeCheck() ? 1 : 0,
		(int)nzbInfo->GetDupeMode(),
		(int)dmForce,
		DupeCoordinator::FailsOver(nzbInfo) ? 1 : 0,
		(int)nzbInfo->GetDeleteStatus(),
		(int)NzbInfo::dsNone,
		(int)NzbInfo::dsHealth,
		(int)NzbInfo::dsBad,
		(int)NzbInfo::dsScan,
		(int)NzbInfo::dsCopy,
		&addToHistoryInt,
		&callDupeCompletedInt,
		&enqueueDeletedScriptInt,
		&needSaveInt);
	bool addToHistory = addToHistoryInt != 0;
	if (addToHistory)
	{
		g_HistoryCoordinator->AddToHistory(downloadQueue, nzbInfo);
	}
	nzbInfo->SetAvoidHistory(false);
	bool needSave = needSaveInt != 0;
	if (callDupeCompletedInt)
	{
		g_DupeCoordinator->NzbCompleted(downloadQueue, nzbInfo);
		needSave = true;
	}
	if (enqueueDeletedScriptInt)
	{
		g_QueueScriptCoordinator->EnqueueScript(nzbInfo, QueueScriptCoordinator::qeNzbDeleted);
	}

	if (!addToHistory)
	{
		g_HistoryCoordinator->DeleteDiskFiles(nzbInfo);
		ParDamage::Forget(nzbInfo->GetId());
		downloadQueue->GetQueue()->Remove(nzbInfo);
		// the removal must reach the disk: the saved queue may hold the download
		// with its files already flagged deleted, which a restart before the next
		// save brought back as a queued download without files (B42)
		needSave = true;
	}

	if (saveQueue && needSave)
	{
		downloadQueue->Save();
	}
}

inline void PrePostProcessor::DeleteCleanup(NzbInfo* nzbInfo)
{
	int cleanupMode = nzbget_rs_prepost_classify_delete_cleanup_mode(
		nzbInfo->GetCleanupDisk() ? 1 : 0,
		(int)nzbInfo->GetDeleteStatus(),
		(int)NzbInfo::dsDupe,
		nzbInfo->GetParking() ? 1 : 0,
		nzbInfo->GetSkipDiskWrite() ? 1 : 0,
		g_Options->GetSkipWrite() ? 1 : 0);
	if (cleanupMode != 0)
	{
		if (cleanupMode == 1)
		{
			if (FileSystem::DirectoryExists(nzbInfo->GetDestDir()))
			{
				CString errmsg;
				detail("Deleting dir %s", nzbInfo->GetDestDir());
				if (!FileSystem::DeleteDirectoryWithContent(nzbInfo->GetDestDir(), errmsg))
				{
					error("Could not delete directory %s: %s", nzbInfo->GetDestDir(), *errmsg);
				}
			}
			return;
		}

		// download was cancelled, deleting already downloaded files from disk
		for (CompletedFile& completedFile: nzbInfo->GetCompletedFiles())
		{
			NzbgetRsBuf fullFnBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
				nzbInfo->GetDestDir(), PATH_SEPARATOR, completedFile.GetFilename());
			BString<1024> fullFileName;
			fullFileName.Set(fullFnBuf.data ? fullFnBuf.data : "", (int)fullFnBuf.len);
			nzbget_rs_free(fullFnBuf);
			if (FileSystem::FileExists(fullFileName))
			{
				detail("Deleting file %s", completedFile.GetFilename());
				FileSystem::DeleteFile(fullFileName);
			}
		}

		// delete .out.tmp-files
		DirBrowser dir(nzbInfo->GetDestDir());
		while (const char* filename = dir.Next())
		{
			size_t fnLen = strlen(filename);
			if (fnLen >= 8 && !strcmp(filename + fnLen - 8, ".out.tmp"))
			{
				NzbgetRsBuf fullFnBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
					nzbInfo->GetDestDir(), PATH_SEPARATOR, filename);
				BString<1024> fullFilename;
				fullFilename.Set(fullFnBuf.data ? fullFnBuf.data : "", (int)fullFnBuf.len);
				nzbget_rs_free(fullFnBuf);
				detail("Deleting file %s", filename);
				FileSystem::DeleteFile(fullFilename);
			}
		}

		// delete old directory (if empty)
		FileSystem::DeleteDirectory(nzbInfo->GetDestDir());

		// delete final directory (if empty)
		const auto finalDir = nzbInfo->BuildFinalDirName();
		if (nzbInfo->GetDestDir() != finalDir && FileSystem::DirectoryExists(finalDir))
		{
			FileSystem::DeleteDirectory(finalDir);
		}
	}
}

inline void PrePostProcessor::CheckRequestPar(DownloadQueue* downloadQueue)
{
#ifndef DISABLE_PARCHECK
	std::lock_guard<std::recursive_mutex> guard(m_activeJobsMutex);
	for (NzbInfo* postJob : m_activeJobs)
	{
		PostInfo* postInfo = postJob->GetPostInfo();

		int newForceParFull = postInfo->GetForceParFull() ? 1 : 0;
		int reqAction = nzbget_rs_prepost_classify_request_par_action(
			postInfo->GetRequestParCheck() ? 1 : 0,
			(int)postInfo->GetNzbInfo()->GetParStatus(),
			(int)NzbInfo::psSkipped,
			postInfo->GetForceRepair() ? 1 : 0,
			postInfo->GetNzbInfo()->GetParFull() ? 1 : 0,
			(int)g_Options->GetParCheck(),
			(int)Options::pcManual,
			newForceParFull,
			postInfo->GetNzbInfo()->GetFileList()->empty() ? 1 : 0,
			&newForceParFull);
		if (reqAction == 1)
		{
			postInfo->SetForceParFull(newForceParFull != 0);
			postInfo->GetNzbInfo()->SetParStatus(NzbInfo::psNone);
			postInfo->SetRequestParCheck(false);
			postInfo->GetNzbInfo()->GetScriptStatuses()->clear();
			postInfo->SetStage(PostInfo::ptQueued);
			postInfo->SetWorking(false);
		}
		else if (reqAction == 2 || reqAction == 3)
		{
			postInfo->SetRequestParCheck(false);
			postInfo->GetNzbInfo()->SetParStatus(NzbInfo::psManual);

			if (reqAction == 2)
			{
				postInfo->GetNzbInfo()->PrintMessage(Message::mkInfo,
					"Downloading all remaining files for manual par-check for %s", postInfo->GetNzbInfo()->GetName());
				downloadQueue->EditEntry(postInfo->GetNzbInfo()->GetId(), DownloadQueue::eaGroupResume, nullptr);
				postInfo->SetStage(PostInfo::ptFinished);
			}
			else
			{
				postInfo->GetNzbInfo()->PrintMessage(Message::mkInfo,
					"There are no par-files remain for download for %s", postInfo->GetNzbInfo()->GetName());
			}
			postInfo->SetWorking(false);
		}
	}
#endif
}

inline void PrePostProcessor::CleanupJobs(DownloadQueue* downloadQueue)
{
	std::lock_guard<std::recursive_mutex> guard(m_activeJobsMutex);
	m_activeJobs.erase(std::remove_if(m_activeJobs.begin(), m_activeJobs.end(),
		[processor = this, downloadQueue](NzbInfo* postJob)
		{
			PostInfo* postInfo = postJob->GetPostInfo();
			bool hasRunningThread = postInfo->GetPostThread() && postInfo->GetPostThread()->IsRunning();
			int64_t curTime = (!postInfo->GetWorking() && !hasRunningThread && postInfo->GetStartTime() > 0)
				? (int64_t)Util::CurrentTime()
				: 0;
			int updateTotalSec = 0;
			int isCompleted = 0;
			int newTotalSec = postJob->GetPostTotalSec();
			if (nzbget_rs_prepost_eval_cleanup_job_state(
				postInfo->GetWorking() ? 1 : 0,
				hasRunningThread ? 1 : 0,
				(int)postInfo->GetStage(),
				(int)PostInfo::ptFinished,
				postInfo->GetDeleted() ? 1 : 0,
				(int64_t)postInfo->GetStartTime(),
				curTime,
				postJob->GetPostTotalSec(),
				&updateTotalSec,
				&isCompleted,
				&newTotalSec) != 0)
			{
				delete postInfo->GetPostThread();
				postInfo->SetPostThread(nullptr);

				postInfo->SetStageTime(0);
				postInfo->SetStageProgress(0);
				postInfo->SetFileProgress(0);
				postInfo->SetProgressLabel("");

				if (updateTotalSec != 0)
				{
					postJob->SetPostTotalSec(newTotalSec);
					postInfo->SetStartTime(0);
				}

				if (isCompleted != 0)
				{
					processor->JobCompleted(downloadQueue, postInfo);
				}
				else
				{
					postInfo->SetStage(PostInfo::ptQueued);
				}
				return true;
			}
			return false;
		}),
		m_activeJobs.end());
}

inline bool PrePostProcessor::CanRunMoreJobs(bool* allowPar)
{
	std::lock_guard<std::recursive_mutex> guard(m_activeJobsMutex);
	std::vector<int> stages;
	stages.reserve(m_activeJobs.size());
	for (NzbInfo* postJob : m_activeJobs)
	{
		stages.push_back((int)postJob->GetPostInfo()->GetStage());
	}
	int allowParInt = 0;
	int canRun = nzbget_rs_prepost_can_run_more_jobs(
		stages.data(),
		stages.size(),
		(int)PostInfo::ptLoadingPars,
		(int)PostInfo::ptVerifyingRepaired,
		(int)PostInfo::ptRepairing,
		(int)g_Options->GetPostStrategy(),
		&allowParInt);
	*allowPar = allowParInt != 0;
	return canRun != 0;
}

inline NzbInfo* PrePostProcessor::PickNextJob(DownloadQueue* downloadQueue, bool allowPar)
{
	NzbInfo* nzbInfo = nullptr;
	std::lock_guard<std::recursive_mutex> guard(m_activeJobsMutex);

	bool hasActiveUnpack = HasActiveUnpack(downloadQueue);

	std::vector<NzbInfo*> items;
	std::vector<NzbgetRsPostJobCandidate> candidates;
	items.reserve(downloadQueue->GetQueue()->size());
	candidates.reserve(downloadQueue->GetQueue()->size());
	for (NzbInfo* nzbInfo1 : downloadQueue->GetQueue())
	{
		items.push_back(nzbInfo1);
		NzbgetRsPostJobCandidate c{};
		PostInfo* pi = nzbInfo1->GetPostInfo();
		c.has_post_info = pi ? 1 : 0;
		c.working = (pi && pi->GetWorking()) ? 1 : 0;
		c.direct_unpack_running = nzbInfo1->GetDirectUnpackStatus() == NzbInfo::nsRunning ? 1 : 0;
		c.has_live_repair_thread = nzbInfo1->GetLiveRepairThread() != nullptr ? 1 : 0;
		c.priority = nzbInfo1->GetPriority();
		c.force_priority = nzbInfo1->GetForcePriority() ? 1 : 0;
		c.need_par_check = (pi && pi->GetNeedParCheck()) ? 1 : 0;
		c.need_unpack = (pi && pi->GetNeedUnpack()) ? 1 : 0;
		if (pi && !pi->GetWorking() && !c.direct_unpack_running && !c.has_live_repair_thread)
		{
			c.has_queue_script_job = g_QueueScriptCoordinator->HasJob(nzbInfo1->GetId(), nullptr) ? 1 : 0;
			c.in_active_jobs = std::find(m_activeJobs.begin(), m_activeJobs.end(), nzbInfo1) != m_activeJobs.end() ? 1 : 0;
			c.download_completed = nzbInfo1->IsDownloadCompleted(true) ? 1 : 0;
		}
		candidates.push_back(c);
	}
	int picked = nzbget_rs_prepost_pick_next_post_job(
		candidates.data(),
		candidates.size(),
		allowPar ? 1 : 0,
		hasActiveUnpack ? 1 : 0,
		g_WorkState->GetPausePostProcess() ? 1 : 0);
	if (picked >= 0 && (size_t)picked < items.size())
	{
		nzbInfo = items[(size_t)picked];
	}

	return nzbInfo;
}

inline void PrePostProcessor::CheckPostQueue()
{
	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();

	size_t countBefore;
	{
		std::lock_guard<std::recursive_mutex> guard(m_activeJobsMutex);
		countBefore = m_activeJobs.size();
	}
	CheckRequestPar(downloadQueue);
	CleanupJobs(downloadQueue);
	bool changed;
	{
		std::lock_guard<std::recursive_mutex> guard(m_activeJobsMutex);
		changed = m_activeJobs.size() != countBefore;
	}

	bool allowPar;
	while (CanRunMoreJobs(&allowPar) && !IsStopped())
	{
		NzbInfo* postJob = PickNextJob(downloadQueue, allowPar);
		if (!postJob)
		{
			break;
		}

		{
			std::lock_guard<std::recursive_mutex> guard(m_activeJobsMutex);
			m_activeJobs.push_back(postJob);
		}

		PostInfo* postInfo = postJob->GetPostInfo();
		if (postInfo->GetStage() == PostInfo::ptQueued &&
			(!g_WorkState->GetPausePostProcess() || postInfo->GetNzbInfo()->GetForcePriority()))
		{
			StartJob(downloadQueue, postInfo, allowPar);
			CheckRequestPar(downloadQueue);
			CleanupJobs(downloadQueue);
			changed = true;
		}
	}

	if (changed)
	{
		downloadQueue->Save();
		UpdatePauseState();
	}

	{
		std::lock_guard<std::recursive_mutex> guard(m_activeJobsMutex);
		Util::SetStandByMode(m_activeJobs.empty());
	}
}

#ifndef DISABLE_PARCHECK
// The collection's par2 files provably cannot cover its damage: the bytes
// still missing from the files they protect exceed all par2 data still
// intact. Health below critical says the same for the whole collection, but
// it also counts damage in files outside the par2 set and never drops for
// partly repaired files, so it only gates the closer look: the remaining
// holes of every stream-repair job whose file the main par2 file lists.
// Several par sets can't be judged from collection-wide sizes, so they are
// always left to par-check.
static inline bool ParCannotCover(NzbInfo* nzbInfo)
{
	int health = nzbInfo->CalcHealth();
	int criticalHealth = nzbInfo->CalcCriticalHealth(false);
	if (health == 0 || health >= criticalHealth)
	{
		return false;
	}

	ParParser::ParFileList mainPars;
	bool foundMainPars = ParParser::FindMainPars(nzbInfo->GetDestDir(), &mainPars);
	if (!foundMainPars || mainPars.size() != 1)
	{
		return false;
	}

	// FindMainPars reports whichever file of the set it meets first; the
	// description packets are cheapest to read from its smallest file (the
	// index, unless every volume was downloaded)
	std::string parPath;
	int64 parFileSize = -1;
	DirBrowser dir(nzbInfo->GetDestDir());
	while (const char* filename = dir.Next())
	{
		if (ParParser::SameParCollection(filename, mainPars[0].c_str(), true))
		{
			NzbgetRsBuf pathBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
				nzbInfo->GetDestDir(), PATH_SEPARATOR, filename);
			BString<1024> path;
			path.Set(pathBuf.data ? pathBuf.data : "", (int)pathBuf.len);
			nzbget_rs_free(pathBuf);
			int64 size = FileSystem::FileSize(path);
			if (size >= 0 && (parFileSize < 0 || size < parFileSize))
			{
				parPath = *path;
				parFileSize = size;
			}
		}
	}

	std::vector<std::string> parredFiles;
	if (parFileSize < 0 || !ParParser::ListParredFiles(parPath.c_str(), parredFiles))
	{
		return false;
	}

	int64 protectedDamage = 0;
	for (StreamRepairJob& job : *nzbInfo->GetStreamRepairJobs())
	{
		std::string filename = job.GetFilename();
		for (CompletedFile& completedFile : nzbInfo->GetCompletedFiles())
		{
			if (completedFile.GetId() == job.GetFileId())
			{
				filename = completedFile.GetFilename();
				break;
			}
		}
		const char* baseName = FileSystem::BaseFileName(filename.c_str());
		if (std::any_of(parredFiles.begin(), parredFiles.end(),
			[baseName](const std::string& parredFile) { return nzbget_rs_dqh_bytes_eq_ignore_case(parredFile.c_str(), baseName) != 0; }))
		{
			// a whole-file job has no decoded geometry yet: its encoded size
			// is the closest measure of its damage
			if (job.GetDecodedFileSize() > 0)
			{
				protectedDamage += DupeStreamRepair::TotalSize(*job.GetHoles());
			}
			else
			{
				protectedDamage += job.GetFailedSize() + job.GetMissedSize();
			}
		}
	}

	int64 intactPar = std::max<int64>(0, nzbInfo->GetParSize() - nzbInfo->GetParCurrentFailedSize());
	return protectedDamage > intactPar;
}

// ParScan=dupe can still find missing blocks in the files of a duplicate
// already downloaded to disk, which the health estimate does not know about
static inline bool HasDupeScanSources(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
{
	if (g_Options->GetParScan() != Options::psDupe)
	{
		return false;
	}
	for (NzbInfo* dupeNzbInfo : g_DupeCoordinator->ListHistoryDupes(downloadQueue, nzbInfo))
	{
		if (!Util::EmptyStr(dupeNzbInfo->GetDestDir()) &&
			FileSystem::DirectoryExists(dupeNzbInfo->GetDestDir()))
		{
			return true;
		}
	}
	return false;
}
#endif

inline void PrePostProcessor::StartJob(DownloadQueue* downloadQueue, PostInfo* postInfo, bool allowPar)
{
	NzbInfo* nzbInfo = postInfo->GetNzbInfo();

	if (!postInfo->GetStartTime())
	{
		postInfo->SetStartTime(Util::CurrentTime());
	}
	postInfo->SetStageTime(Util::CurrentTime());
	postInfo->SetStageProgress(0);
	postInfo->SetFileProgress(0);
	postInfo->SetProgressLabel("");

	if (nzbInfo->GetParRenameStatus() == NzbInfo::rsNone &&
		nzbInfo->GetDeleteStatus() == NzbInfo::dsNone &&
		g_Options->GetParRename())
	{
		EnterStage(downloadQueue, postInfo, PostInfo::ptParRenaming);
		RenameController::StartJob(postInfo, RenameController::jkPar);
		return;
	}

	bool streamRepair = g_Options->GetDupeArticleFallback() >= Options::dafStream &&
		!nzbInfo->GetStreamRepairJobs()->empty() &&
		nzbInfo->GetDeleteStatus() == NzbInfo::dsNone;

	// stream repair is the last option (B78): it can take longer than downloading
	// a good backup whole, so with HealthCheck=dupe and a backup still untried in
	// history the download fails over instead (par2 still gets its turn)
	if (streamRepair && !postInfo->GetStreamRepairDone() &&
		g_Options->GetHealthCheck() == Options::hcDupe &&
		DupeCoordinator::FailsOver(nzbInfo) &&
		g_DupeCoordinator->FindDupeBackup(downloadQueue, nzbInfo, nzbInfo->GetName(), nzbInfo->GetDupeKey()))
	{
		nzbInfo->PrintMessage(Message::mkInfo,
			"Not repairing %s from duplicates: a backup in history is tried first", nzbInfo->GetName());
		streamRepair = false;
	}

#ifndef DISABLE_PARCHECK
	// A par-check that cannot succeed still reads the whole collection (and
	// waits for every remaining par2-file): recover from duplicates first,
	// par-check afterwards only if that wrote anything
	if (streamRepair && !postInfo->GetStreamRepairDone() &&
		nzbInfo->GetParStatus() == NzbInfo::psSkipped &&
		g_Options->GetParCheck() == Options::pcAuto &&
		ParCannotCover(nzbInfo) &&
		ParParser::FindMainPars(nzbInfo->GetDestDir(), nullptr))
	{
		nzbInfo->PrintMessage(Message::mkInfo,
			"Par2 files of %s cannot cover its damage (health %.1f%%, critical %.1f%%), "
			"recovering from duplicates before par-check",
			nzbInfo->GetName(),
			nzbInfo->CalcHealth() / 10.0,
			nzbInfo->CalcCriticalHealth(false) / 10.0);
		postInfo->SetParCannotCover(true);
		EnterStage(downloadQueue, postInfo, PostInfo::ptStreamRepairing);
		StreamRepairController::StartJob(postInfo);
		return;
	}

	// Duplicates recovered nothing, so the damage is still beyond the par2
	// files: report the failure instead of verifying a collection that
	// cannot be repaired. Any byte ever recovered from duplicates (a live
	// pass, or a pass interrupted by a restart) may have brought the damage
	// within reach, so then par-check still runs.
	if (postInfo->GetParCannotCover() && postInfo->GetStreamRepairDone() &&
		!postInfo->GetStreamRepairRecovered() &&
		nzbInfo->GetDupeRecoveredBytes() <= 0 &&
		g_Options->GetParCheck() == Options::pcAuto &&
		nzbInfo->GetParStatus() <= NzbInfo::psNone &&
		nzbInfo->GetDeleteStatus() == NzbInfo::dsNone &&
		!HasDupeScanSources(downloadQueue, nzbInfo) &&
		ParParser::FindMainPars(nzbInfo->GetDestDir(), nullptr))
	{
		nzbInfo->PrintMessage(Message::mkWarning,
			"Skipping par-check for %s: its par2 files cannot cover the damage "
			"(health %.1f%%, critical %.1f%%) and duplicates recovered nothing",
			nzbInfo->GetName(),
			nzbInfo->CalcHealth() / 10.0,
			nzbInfo->CalcCriticalHealth(false) / 10.0);
		nzbInfo->SetParStatus(NzbInfo::psFailure);
		return;
	}

	// Captured holes are a fallback, not evidence that donor data is needed.
	// Let the current PAR set verify and repair first, including auto mode
	// which initially marks the check as skipped until damage is known.
	if (streamRepair && nzbInfo->GetParStatus() == NzbInfo::psSkipped &&
		ParParser::FindMainPars(nzbInfo->GetDestDir(), nullptr))
	{
		postInfo->SetRequestParCheck(true);
		return;
	}

	// damaged, and the par2-files downloaded so far all failed (a lost index file):
	// any par2-volume carries the set's description too, so the smallest one held
	// back is fetched, and the job waits for it (it isn't picked while files download)
	if ((nzbInfo->GetParStatus() == NzbInfo::psNone || nzbInfo->GetParStatus() == NzbInfo::psSkipped) &&
		nzbInfo->GetDeleteStatus() == NzbInfo::dsNone &&
		nzbInfo->CalcHealth() < 1000 &&
		!ParParser::FindMainPars(nzbInfo->GetDestDir(), nullptr))
	{
		FileInfo* smallest = nullptr;
		for (FileInfo* fileInfo : nzbInfo->GetFileList())
		{
			if (fileInfo->GetParFile() && fileInfo->GetPaused() && !fileInfo->GetDeleted() &&
				(!smallest || fileInfo->GetSize() < smallest->GetSize()))
			{
				smallest = fileInfo;
			}
		}
		if (smallest)
		{
			nzbInfo->PrintMessage(Message::mkInfo,
				"No par2-file of %s on disk to check it with: unpausing %s", nzbInfo->GetName(), smallest->GetFilename());
			smallest->SetPaused(false);
			smallest->SetExtraPriority(true);
			nzbInfo->SetChanged(true);
			downloadQueue->SaveChanged();
			return;
		}
	}

	if (nzbInfo->GetParStatus() == NzbInfo::psNone &&
		nzbInfo->GetDeleteStatus() == NzbInfo::dsNone)
	{
		if (ParParser::FindMainPars(nzbInfo->GetDestDir(), nullptr))
		{
			if (!allowPar)
			{
				postInfo->SetNeedParCheck(true);
				return;
			}

			EnterStage(downloadQueue, postInfo, PostInfo::ptLoadingPars);
			postInfo->SetNeedParCheck(false);
			RepairController::StartJob(postInfo);
		}
		else
		{
			nzbInfo->PrintMessage(Message::mkInfo,
				"Nothing to par-check for %s", nzbInfo->GetName());
			nzbInfo->SetParStatus(NzbInfo::psSkipped);
		}
		return;
	}
	
	int skippedParAction = nzbget_rs_prepost_classify_skipped_par_health(
		(int)nzbInfo->GetParStatus(),
		(int)NzbInfo::psSkipped,
		(int)g_Options->GetParScan(),
		(int)Options::psDupe,
		nzbInfo->CalcHealth(),
		nzbInfo->CalcCriticalHealth(false),
		nzbInfo->GetFailedSize(),
		nzbInfo->GetParFailedSize());
	if ((skippedParAction == 1 || skippedParAction == 2) &&
		ParParser::FindMainPars(nzbInfo->GetDestDir(), nullptr))
	{
		if (skippedParAction == 1)
		{
			nzbInfo->PrintMessage(Message::mkWarning,
				"Skipping par-check for %s due to health 0%%", nzbInfo->GetName());
		}
		else
		{
			nzbInfo->PrintMessage(Message::mkWarning,
				"Skipping par-check for %s due to health %.1f%% below critical %.1f%%",
				nzbInfo->GetName(),
				nzbInfo->CalcHealth() / 10.0,
				nzbInfo->CalcCriticalHealth(false) / 10.0);
		}
		nzbInfo->SetParStatus(NzbInfo::psFailure);
		return;
	}
	if (skippedParAction == 3 &&
		ParParser::FindMainPars(nzbInfo->GetDestDir(), nullptr))
	{
		nzbInfo->PrintMessage(Message::mkInfo,
			"Collection %s with health %.1f%% needs par-check",
			nzbInfo->GetName(),
			nzbInfo->CalcHealth() / 10.0);
		postInfo->SetRequestParCheck(true);
		return;
	}
#endif

	// Successful PAR sets remove the holes they actually protect. Remaining
	// jobs may belong to unprotected files, or to a set that could not repair.
	// Manual repair and sufficient parity with ParRepair=no remain user choices.
	if (streamRepair &&
		nzbInfo->GetParStatus() != NzbInfo::psRepairPossible &&
		nzbInfo->GetParStatus() != NzbInfo::psManual)
	{
		EnterStage(downloadQueue, postInfo, PostInfo::ptStreamRepairing);
		StreamRepairController::StartJob(postInfo);
		return;
	}

	NzbParameter* unpackParameter = nzbInfo->GetParameters()->Find("*Unpack:");
	int wantUnpackInt = 0;
	int unpackInt = 0;
	bool startRarRename = nzbget_rs_prepost_eval_want_unpack_and_rar_rename(
		unpackParameter != nullptr ? 1 : 0,
		unpackParameter ? unpackParameter->GetValue() : "",
		(int)nzbInfo->GetUnpackStatus(),
		(int)NzbInfo::usNone,
		(int)nzbInfo->GetDeleteStatus(),
		(int)NzbInfo::dsNone,
		(int)nzbInfo->GetRarRenameStatus(),
		(int)NzbInfo::rsNone,
		g_Options->GetRarRename() ? 1 : 0,
		&wantUnpackInt,
		&unpackInt) != 0;
	bool wantUnpack = wantUnpackInt != 0;
	bool unpack = unpackInt != 0;
	if (startRarRename)
	{
		EnterStage(downloadQueue, postInfo, PostInfo::ptRarRenaming);
		RenameController::StartJob(postInfo, RenameController::jkRar);
		return;
	}

#ifndef DISABLE_PARCHECK
	if (nzbInfo->GetParStatus() == NzbInfo::psSkipped &&
		nzbInfo->GetDeleteStatus() == NzbInfo::dsNone &&
		g_Options->GetParCheck() == Options::pcAuto &&
		!UnpackController::HasCompletedArchiveFiles(nzbInfo) &&
		ParParser::FindMainPars(nzbInfo->GetDestDir(), nullptr))
	{
		nzbInfo->PrintMessage(Message::mkInfo,
			"Requesting par-check for collection %s without archive files",
			nzbInfo->GetName());
		postInfo->SetRequestParCheck(true);
		return;
	}
#endif

	NzbgetRsPostStageDecision stageDec{};
	nzbget_rs_prepost_eval_post_stages(
		wantUnpack ? 1 : 0,
		(int)nzbInfo->GetUnpackStatus(),
		(int)nzbInfo->GetDeleteStatus(),
		(int)nzbInfo->GetParStatus(),
		(int)nzbInfo->GetCleanupStatus(),
		!Util::EmptyStr(g_Options->GetExtCleanupDisk()) ? 1 : 0,
		nzbInfo->CalcHealth(),
		(int)nzbInfo->GetMoveStatus(),
		nzbInfo->GetDestDir(),
		g_Options->GetInterDir(),
		PATH_SEPARATOR,
		(int)nzbInfo->GetDirectUnpackStatus(),
		g_Options->GetPostDownloadRename() ? 1 : 0,
		(int)nzbInfo->GetPostDownloadRenamingStatus(),
		!Util::EmptyStr(nzbInfo->GetDestDir()) ? 1 : 0,
		!Util::EmptyStr(nzbInfo->GetName()) ? 1 : 0,
		g_Options->GetRenameAfterUnpack() ? 1 : 0,
		(int)nzbInfo->GetPostUnpackRenamingStatus(),
		&stageDec);
	if (stageDec.skip_unpack_due_to_par)
	{
		nzbInfo->PrintMessage(Message::mkWarning,
			"Skipping unpack for %s due to %s", nzbInfo->GetName(),
			nzbInfo->GetParStatus() == NzbInfo::psManual ? "required par-repair" : "par-failure");
		nzbInfo->SetUnpackStatus(NzbInfo::usSkipped);
	}
	unpack = stageDec.unpack != 0;
	bool cleanup = stageDec.cleanup != 0;
	bool moveInter = stageDec.move_inter != 0;
	bool postDownloadRenaming = stageDec.post_download_renaming != 0;
	bool postUnpackRenaming = stageDec.post_unpack_renaming != 0;

	if (unpack)
	{
		if (HasActiveUnpack(downloadQueue))
		{
			postInfo->SetNeedUnpack(true);
			return;
		}
		postInfo->SetNeedUnpack(false);
		EnterStage(downloadQueue, postInfo, PostInfo::ptUnpacking);
		UnpackController::StartJob(postInfo);
	}
	else if (cleanup)
	{
		EnterStage(downloadQueue, postInfo, PostInfo::ptCleaningUp);
		CleanupController::StartJob(postInfo);
	}
	else if (moveInter)
	{
		EnterStage(downloadQueue, postInfo, PostInfo::ptMoving);
		MoveController::StartJob(postInfo);
	}
	else if (postDownloadRenaming)
	{
		EnterStage(downloadQueue, postInfo, PostInfo::ptPostDownloadRenaming);
		PostRenamerController::StartJob(postInfo, false);
	}
	else if (postUnpackRenaming)
	{
		EnterStage(downloadQueue, postInfo, PostInfo::ptPostUnpackRenaming);
		PostRenamerController::StartJob(postInfo, true);
	}
	else
	{
		EnterStage(downloadQueue, postInfo, PostInfo::ptExecutingScript);
		PostScriptController::StartJob(postInfo);
	}
}

inline void PrePostProcessor::EnterStage(DownloadQueue* downloadQueue, PostInfo* postInfo, PostInfo::EStage stage)
{
	postInfo->SetWorking(true);
	postInfo->SetStage(stage);
}

inline void PrePostProcessor::JobCompleted(DownloadQueue* downloadQueue, PostInfo* postInfo)
{
	NzbInfo* nzbInfo = postInfo->GetNzbInfo();

	nzbInfo->LeavePostProcess();

	if (nzbInfo->IsDownloadCompleted(true))
	{
		NzbCompleted(downloadQueue, nzbInfo, false);
	}

	std::lock_guard<std::mutex> guard(m_waitMutex);
	m_queuedJobs--;
}

inline void PrePostProcessor::UpdatePauseState()
{
	bool needPause = false;
	std::lock_guard<std::recursive_mutex> guard(m_activeJobsMutex);
	std::vector<int> stages;
	stages.reserve(m_activeJobs.size());
	for (NzbInfo* postJob : m_activeJobs)
	{
		stages.push_back((int)postJob->GetPostInfo()->GetStage());
	}
	needPause = nzbget_rs_prepost_calc_post_pause_state(
		stages.data(),
		stages.size(),
		g_Options->GetParPauseQueue() ? 1 : 0,
		g_Options->GetUnpackPauseQueue() ? 1 : 0,
		g_Options->GetScriptPauseQueue() ? 1 : 0) != 0;

	if (needPause && !g_WorkState->GetTempPauseDownload())
	{
		info("Pausing download before post-processing");
	}
	else if (!needPause && g_WorkState->GetTempPauseDownload())
	{
		info("Unpausing download after post-processing");
	}

	g_WorkState->SetTempPauseDownload(needPause);
}

inline bool PrePostProcessor::EditList(DownloadQueue* downloadQueue, IdList* idList,
	DownloadQueue::EEditAction action, const char* args)
{
	debug("Edit-command for post-processor received");
	switch (action)
	{
		case DownloadQueue::eaPostDelete:
			return PostQueueDelete(downloadQueue, idList);

		default:
			return false;
	}
}

inline bool PrePostProcessor::PostQueueDelete(DownloadQueue* downloadQueue, IdList* idList)
{
	bool ok = false;

	for (int id : *idList)
	{
		for (NzbInfo* nzbInfo: downloadQueue->GetQueue())
		{
			PostInfo* postInfo = nzbInfo->GetPostInfo();
			if (postInfo && nzbInfo->GetId() == id)
			{
				if (postInfo->GetWorking())
				{
					postInfo->GetNzbInfo()->PrintMessage(Message::mkInfo,
						"Deleting active post-job %s", postInfo->GetNzbInfo()->GetName());
					postInfo->SetDeleted(true);
					if (postInfo->GetPostThread())
					{
						debug("Terminating post-process thread for %s", postInfo->GetNzbInfo()->GetName());
						postInfo->GetPostThread()->Stop();
						ok = true;
					}
					else if (postInfo->GetNzbInfo()->GetUnpackThread())
					{
						((DirectUnpack*)postInfo->GetNzbInfo()->GetUnpackThread())->NzbDeleted(downloadQueue, postInfo->GetNzbInfo());
						ok = true;
					}
					else if (postInfo->GetNzbInfo()->GetLiveRepairThread())
					{
						// the job is waiting for a live stream-repair pass:
						// detach first (the pass aborts at its next locked
						// touchpoint without writing back), stop its fetches
						// and release the job
						Thread* liveRepairThread = postInfo->GetNzbInfo()->GetLiveRepairThread();
						postInfo->GetNzbInfo()->SetLiveRepairThread(nullptr);
						liveRepairThread->Stop();
						postInfo->SetWorking(false);
						ok = true;
					}
					else
					{
						error("Internal error in PrePostProcessor::QueueDelete");
					}
				}
				else
				{
					postInfo->GetNzbInfo()->PrintMessage(Message::mkInfo,
						"Deleting queued post-job %s", postInfo->GetNzbInfo()->GetName());

					JobCompleted(downloadQueue, postInfo);

					{
						std::lock_guard<std::recursive_mutex> guard(m_activeJobsMutex);
						m_activeJobs.erase(std::remove_if(m_activeJobs.begin(), m_activeJobs.end(),
							[postInfo](NzbInfo* postJob)
							{
								return postInfo == postJob->GetPostInfo();
							}),
							m_activeJobs.end());
					}

					ok = true;
				}
				break;
			}
		}
	}

	if (ok)
	{
		downloadQueue->Save();
	}

	return ok;
}

inline void PrePostProcessor::FileDownloaded(DownloadQueue* downloadQueue, NzbInfo* nzbInfo, FileInfo* fileInfo)
{
	if (!nzbInfo->GetPostInfo())
	{
		g_QueueScriptCoordinator->EnqueueScript(nzbInfo, QueueScriptCoordinator::qeFileDownloaded);
	}

	int duAction = nzbget_rs_prepost_classify_file_downloaded_direct_unpack(
		g_Options->GetDirectUnpack() ? 1 : 0,
		g_Options->GetRawArticle() ? 1 : 0,
		g_Options->GetSkipWrite() ? 1 : 0,
		nzbInfo->GetSkipDiskWrite() ? 1 : 0,
		(int)nzbInfo->GetDirectUnpackStatus(),
		(int)NzbInfo::nsNone,
		(int)NzbInfo::nsRunning,
		(int)nzbInfo->GetDirectRenameStatus(),
		(int)NzbInfo::tsRunning,
		nzbInfo->GetUnpackThread() != nullptr ? 1 : 0);
	if (duAction == 1)
	{
		bool allowPar;
		if (DirectUnpack::IsArchiveFilename(fileInfo->GetFilename()) &&
			CanRunMoreJobs(&allowPar) &&
			!HasActiveUnpack(downloadQueue))
		{
			NzbParameter* unpackParameter = nzbInfo->GetParameters()->Find("*Unpack:");
			bool wantUnpack = !unpackParameter || !strcasecmp(unpackParameter->GetValue(), "yes");
			if (wantUnpack && nzbInfo->GetFailedArticles() == 0)
			{
				DirectUnpack::StartJob(nzbInfo);
			}
		}
	}
	else if (duAction == 2)
	{
		((DirectUnpack*)nzbInfo->GetUnpackThread())->FileDownloaded(downloadQueue, fileInfo);
	}
}

#endif
