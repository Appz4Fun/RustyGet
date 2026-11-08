/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2013-2018 Andrey Prygunkov <hugbug@users.sourceforge.net>
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
 *  along with this program.  If not, see <https://www.gnu.org/licenses/>.
 */


#ifndef UNPACK_CONTROLLER_H
#define UNPACK_CONTROLLER_H

#include "Log.h"
#include "Thread.h"
#include "DownloadInfo.h"
#include "ScriptController.h"

class UnpackController final : public Thread, public ScriptController
{
public:
	void Run() override;
	void Stop() override;
	static void StartJob(PostInfo* postInfo);
	static bool HasCompletedArchiveFiles(NzbInfo* nzbInfo);
	static Mutex& GetUnpackMutex();
	bool IsUnpacking() const { return m_unpacking; }

	~UnpackController() override;

protected:
	void AddMessage(Message::EKind kind, const char* text) override;

private:
	enum EUnpacker
	{
		upUnrar,
		upSevenZip
	};

	typedef std::vector<CString> FileList;

	PostInfo* m_postInfo = nullptr;
	CString m_name;
	CString m_infoName;
	CString m_infoNameUp;
	CString m_destDir;
	CString m_finalDir;
	CString m_unpackDir;
	CString m_unpackExtendedDir;
	CString m_password;
	EUnpacker m_unpacker;
	bool m_interDir = false;
	bool m_allOkMessageReceived = false;
	bool m_noFilesMessageReceived = false;
	bool m_hasParFiles = false;
	bool m_hasRarFiles = false;
	bool m_hasNotUnpackedRarFiles = false;
	bool m_hasRenamedArchiveFiles = false;
	bool m_hasSevenZipFiles = false;
	bool m_hasSevenZipMultiFiles = false;
	bool m_hasSplittedFiles = false;
	bool m_unpackOk = false;
	bool m_unpackStartError = false;
	bool m_unpackSpaceError = false;
	bool m_unpackDecryptError = false;
	bool m_unpackPasswordError = false;
	bool m_cleanedUpDisk = false;
	bool m_autoTerminated = false;
	bool m_finalDirCreated = false;
	bool m_unpackDirCreated = false;
	bool m_passListTried = false;
	std::atomic<bool> m_unpacking{false};
	std::atomic<std::thread::id> m_execThreadId{};
	FileList m_joinedFiles;
	FileList m_downloadedFiles;

	void ExecuteUnpack(EUnpacker unpacker, const char* password, bool multiVolumes);
	void UnpackArchives(EUnpacker unpacker, bool multiVolumes);
	void JoinSplittedFiles();
	void Completed();
	void CreateUnpackDir();
	bool Cleanup();
	void CheckArchiveFiles();
	void SetProgressLabel(const char* progressLabel);
#ifndef DISABLE_PARCHECK
	void RequestParCheck(bool forceRepair);
#endif
	bool WillRetryWithPass();
};

#include "Log.h"
#include "Util.h"
#include "FileSystem.h"
#include "ParParser.h"
#include "Options.h"
#include "nzbget_rs.h"

inline UnpackController::~UnpackController()
{
#ifndef DISABLE_TLS
	OpenSSL::StopSSLThread();
#endif
}

inline Mutex& UnpackController::GetUnpackMutex()
{
	static Mutex s_unpackMutex;
	return s_unpackMutex;
}

inline void UnpackController::StartJob(PostInfo* postInfo)
{
	UnpackController* unpackController = new UnpackController();
	unpackController->m_postInfo = postInfo;
	unpackController->SetAutoDestroy(false);

	postInfo->SetPostThread(unpackController);

	unpackController->Start();
}

inline void UnpackController::Run()
{
	time_t start = Util::CurrentTime();
	m_unpackOk = true;
	bool unpack;

	{
		GuardedDownloadQueue guard = DownloadQueue::Guard();

		m_destDir = m_postInfo->GetNzbInfo()->GetDestDir();
		m_finalDir = m_postInfo->GetNzbInfo()->GetFinalDir();
		m_name = m_postInfo->GetNzbInfo()->GetName();

		for (CompletedFile& completedFile : m_postInfo->GetNzbInfo()->GetCompletedFiles())
		{
			m_downloadedFiles.emplace_back(completedFile.GetFilename());
		}

		NzbParameter* parameter = m_postInfo->GetNzbInfo()->GetParameters()->Find("*Unpack:");
		unpack = !(parameter && !strcasecmp(parameter->GetValue(), "no"));

		parameter = m_postInfo->GetNzbInfo()->GetParameters()->Find("*Unpack:Password");
		if (parameter)
		{
			m_password = parameter->GetValue();
		}
	}

	m_infoName.Format("unpack for %s", *m_name);
	m_infoNameUp.Format("Unpack for %s", *m_name);

	m_hasParFiles = ParParser::FindMainPars(m_destDir, nullptr);

	if (unpack)
	{
		CheckArchiveFiles();
	}

	SetInfoName(m_infoName);
	SetWorkingDir(m_destDir);

	bool hasFiles = m_hasRarFiles || m_hasSevenZipFiles || m_hasSevenZipMultiFiles || m_hasSplittedFiles;

	if (m_postInfo->GetUnpackTried() && !m_postInfo->GetParRepaired() &&
		(!m_password.Empty() || Util::EmptyStr(g_Options->GetUnpackPassFile()) || m_postInfo->GetPassListTried()))
	{
		PrintMessage(Message::mkInfo, "Second unpack attempt skipped for %s due to par-check not repaired anything", *m_name);
		PrintMessage(Message::mkError,
			m_postInfo->GetLastUnpackStatus() == (int)NzbInfo::usPassword ?
				"%s failed: checksum error in the encrypted file. Corrupt file or wrong password." :
				"%s failed.",
			*m_infoNameUp);
		m_postInfo->GetNzbInfo()->SetUnpackStatus((NzbInfo::EPostUnpackStatus)m_postInfo->GetLastUnpackStatus());
	}
	else if (unpack && hasFiles)
	{
		PrintMessage(Message::mkInfo, "Unpacking %s", *m_name);

		CreateUnpackDir();

		if (m_hasRarFiles)
		{
			if (m_hasNotUnpackedRarFiles || m_unpackDirCreated)
			{
				if (m_postInfo->GetNzbInfo()->GetDirectUnpackStatus() == NzbInfo::nsSuccess)
				{
					PrintMessage(
						m_unpackDirCreated ? Message::mkWarning : Message::mkInfo,
						m_unpackDirCreated ?
							"Could not find files unpacked by direct unpack, unpacking all files again" :
							"Found archive files not processed by direct unpack, unpacking all files again");
				}

				// Discard info about extracted archives to prevent reusing on next unpack attempt
				m_postInfo->GetExtractedArchives()->clear();

				UnpackArchives(upUnrar, false);
			}
			else
			{
				PrintMessage(Message::mkInfo, "Using directly unpacked files");
			}
		}

		if (m_hasSevenZipFiles && m_unpackOk)
		{
			UnpackArchives(upSevenZip, false);
		}

		if (m_hasSevenZipMultiFiles && m_unpackOk)
		{
			UnpackArchives(upSevenZip, true);
		}

		if (m_hasSplittedFiles && m_unpackOk)
		{
			JoinSplittedFiles();
		}

		Completed();

		m_joinedFiles.clear();
	}
	else if (unpack)
	{
#ifndef DISABLE_PARCHECK
		if (m_postInfo->GetNzbInfo()->GetParStatus() <= NzbInfo::psSkipped &&
			m_postInfo->GetNzbInfo()->GetParRenameStatus() <= NzbInfo::rsSkipped &&
			m_hasParFiles)
		{
			PrintMessage(Message::mkInfo, "Nothing to unpack for %s", *m_name);
			RequestParCheck(false);
		}
		else
#endif
		if (m_hasRenamedArchiveFiles)
		{
			PrintMessage(Message::mkError, "Could not unpack %s due to renamed archive files", *m_name);
			m_postInfo->GetNzbInfo()->SetUnpackStatus(NzbInfo::usFailure);
		}
		else
		{
			PrintMessage(Message::mkInfo, "Nothing to unpack for %s", *m_name);
			m_postInfo->GetNzbInfo()->SetUnpackStatus(NzbInfo::usSkipped);
		}
	}
	else
	{
		PrintMessage(Message::mkInfo, "Unpack for %s skipped", *m_name);
		m_postInfo->GetNzbInfo()->SetUnpackStatus(NzbInfo::usSkipped);
	}

	m_postInfo->GetNzbInfo()->SetUnpackSec(
		m_postInfo->GetNzbInfo()->GetUnpackSec() + (int)(Util::CurrentTime() - start));

	m_postInfo->SetWorking(false);
}

inline void UnpackController::UnpackArchives(EUnpacker unpacker, bool multiVolumes)
{
	if (!m_postInfo->GetUnpackTried() || m_postInfo->GetParRepaired())
	{
		ExecuteUnpack(unpacker, m_password, multiVolumes);
		if (!m_unpackOk && m_hasParFiles && !m_unpackPasswordError &&
			m_postInfo->GetNzbInfo()->GetParStatus() <= NzbInfo::psSkipped)
		{
			debug("For rar4- or 7z-archives try par-check first, before trying password file");
			return;
		}
	}
	else
	{
		m_unpackOk = false;
		m_unpackDecryptError = m_postInfo->GetLastUnpackStatus() == (int)NzbInfo::usPassword;
	}

	if (!m_unpackOk && !m_unpackStartError && !m_unpackSpaceError &&
		(m_unpackDecryptError || m_unpackPasswordError) &&
		(!GetTerminated() || m_autoTerminated) &&
		m_password.Empty() && !Util::EmptyStr(g_Options->GetUnpackPassFile()))
	{
		DiskFile infile;
		if (!infile.Open(g_Options->GetUnpackPassFile(), DiskFile::omRead))
		{
			PrintMessage(Message::mkError, "Could not open file %s", g_Options->GetUnpackPassFile());
			return;
		}

		char password[512];
		while (!m_unpackOk && !m_unpackStartError && !m_unpackSpaceError &&
			(m_unpackDecryptError || m_unpackPasswordError) &&
			infile.ReadLine(password, sizeof(password) - 1))
		{
			debug("Password line: %s", password);
			size_t passLen = strlen(password);
			while (passLen > 0 && (password[passLen - 1] == '\n' || password[passLen - 1] == '\r'))
			{
				password[--passLen] = '\0';
			}
			if (passLen > 0)
			{
				if (IsStopped() && m_autoTerminated)
				{
					ScriptController::Resume();
					Thread::Resume();
				}
				m_unpackDecryptError = false;
				m_unpackPasswordError = false;
				m_autoTerminated = false;
				PrintMessage(Message::mkInfo, "Trying password %s for %s", password, *m_name);
				ExecuteUnpack(unpacker, password, multiVolumes);
			}
		}

		infile.Close();
		m_passListTried = !IsStopped() || m_autoTerminated;
	}
}

inline void UnpackController::ExecuteUnpack(EUnpacker unpacker, const char* password, bool multiVolumes)
{
	const bool isSevenZip = unpacker == upSevenZip;
	if (!isSevenZip)
	{
		m_unpackExtendedDir = FileSystem::MakeExtendedPath(m_unpackDir, true);
	}
	m_allOkMessageReceived = false;
	m_unpackSpaceError = false;
	m_unpacker = unpacker;

	if (isSevenZip)
	{
		PrintMessage(Message::mkInfo, "Executing 7-Zip");
		SetLogPrefix("7-Zip");
	}
	SetProgressLabel("");
	Guard unpackGuard(GetUnpackMutex());
	m_execThreadId = std::this_thread::get_id();
	m_unpacking = true;

	std::vector<const char*> downloadedPtrs;
	downloadedPtrs.reserve(m_downloadedFiles.size());
	for (const CString& f : m_downloadedFiles)
	{
		downloadedPtrs.push_back(*f);
	}

	auto logCb = [](void* ctx, int kind, const char* msg)
	{
		if (!ctx || !msg) return;
		UnpackController* self = static_cast<UnpackController*>(ctx);
		Message::EKind msgKind = Message::mkDetail;
		switch (kind)
		{
			case 0: msgKind = Message::mkInfo; break;
			case 1: msgKind = Message::mkError; break;
			case 2: msgKind = Message::mkWarning; break;
			case 4: msgKind = Message::mkInfo; break;
			default: msgKind = Message::mkDetail; break;
		}
		self->AddMessage(msgKind, msg);
	};

	auto isStoppedCb = [](void* ctx) -> int
	{
		if (!ctx) return 0;
		UnpackController* self = static_cast<UnpackController*>(ctx);
		return (self->IsStopped() || self->GetTerminated()) ? 1 : 0;
	};

	auto resetAllOkCb = [](void* ctx)
	{
		if (ctx) static_cast<UnpackController*>(ctx)->m_allOkMessageReceived = false;
	};

	auto hasAllOkCb = [](void* ctx) -> int
	{
		return (ctx && static_cast<UnpackController*>(ctx)->m_allOkMessageReceived) ? 1 : 0;
	};

	const char* pw = Util::EmptyStr(password) ? "" : password;
	const int cleanupDisk = (g_Options && g_Options->GetUnpackCleanupDisk()) ? 1 : 0;
	const int useTempDir = (g_Options && g_Options->GetUseTempUnpackDir()) ? 1 : 0;
	bool allOk = nzbget_rs_unpack_controller_extract_archives(
		m_destDir,
		m_unpackDir,
		pw,
		isSevenZip ? 1 : 0,
		multiVolumes ? 1 : 0,
		useTempDir,
		cleanupDisk,
		downloadedPtrs.data(),
		downloadedPtrs.size(),
		this,
		resetAllOkCb,
		hasAllOkCb,
		logCb,
		isStoppedCb) != 0;

	m_unpacking = false;
	m_execThreadId = std::thread::id{};
	SetLogPrefix(nullptr);
	SetProgressLabel("");

	m_unpackOk = allOk && m_allOkMessageReceived && !GetTerminated() && !IsStopped();
	m_unpackStartError = false;
}

inline bool UnpackController::WillRetryWithPass()
{
	return (m_unpackDecryptError || m_unpackPasswordError) &&
		m_password.Empty() && !g_Options->GetUnpackPassFilePath().empty();
}

inline void UnpackController::JoinSplittedFiles()
{
	SetLogPrefix("Join");
	SetProgressLabel("");
	m_postInfo->SetStageProgress(0);

	auto logCb = [](void* ctx, int kind, const char* msg)
	{
		if (!ctx || !msg) return;
		UnpackController* self = static_cast<UnpackController*>(ctx);
		Message::EKind msgKind = Message::mkInfo;
		switch (kind)
		{
			case 0: msgKind = Message::mkInfo; break;
			case 1: msgKind = Message::mkError; break;
			case 2: msgKind = Message::mkWarning; break;
			default: msgKind = Message::mkDetail; break;
		}
		self->PrintMessage(msgKind, "%s", msg);
	};

	auto progressCb = [](void* ctx, const char* label, int stageProgress)
	{
		if (!ctx) return;
		UnpackController* self = static_cast<UnpackController*>(ctx);
		if (label)
		{
			self->SetProgressLabel(label);
		}
		if (stageProgress >= 0 && self->m_postInfo)
		{
			self->m_postInfo->SetStageProgress(stageProgress);
		}
	};

	auto addJoinedCb = [](void* ctx, const char* pieceName)
	{
		if (!ctx || !pieceName) return;
		static_cast<UnpackController*>(ctx)->m_joinedFiles.emplace_back(pieceName);
	};

	const int parSuccess = (m_postInfo && m_postInfo->GetNzbInfo() &&
		m_postInfo->GetNzbInfo()->GetParStatus() == NzbInfo::psSuccess) ? 1 : 0;
	const int writeBufferKb = g_Options ? g_Options->GetWriteBuffer() : 0;

	if (nzbget_rs_unpack_controller_join_splitted_files(
		m_destDir, m_unpackDir, parSuccess, writeBufferKb,
		this, logCb, progressCb, addJoinedCb) == 0)
	{
		m_unpackOk = false;
	}

	SetLogPrefix(nullptr);
	SetProgressLabel("");
}

inline void UnpackController::Completed()
{
	if (!m_autoTerminated && !(m_postInfo && m_postInfo->GetDeleted()) &&
		(IsStopped() || GetTerminated()))
	{
		debug("Unpack stopped due to reload/shutdown, preserving _unpack directory");
		return;
	}

	bool cleanupSuccess = Cleanup();

	if (m_unpackOk && cleanupSuccess)
	{
		PrintMessage(Message::mkInfo, "%s %s", *m_infoNameUp, "successful");
		m_postInfo->GetNzbInfo()->SetUnpackStatus(NzbInfo::usSuccess);
		m_postInfo->GetNzbInfo()->SetUnpackCleanedUpDisk(m_cleanedUpDisk);
		if (g_Options->GetParRename())
		{
			//request par-rename check for extracted files
			m_postInfo->GetNzbInfo()->SetParRenameStatus(NzbInfo::rsNone);
		}
	}
	else
	{
#ifndef DISABLE_PARCHECK
		if (!m_unpackOk &&
			(m_postInfo->GetNzbInfo()->GetParStatus() <= NzbInfo::psSkipped ||
			 !m_postInfo->GetNzbInfo()->GetParFull()) &&
			!m_unpackStartError && !m_unpackSpaceError && !m_unpackPasswordError &&
			(!GetTerminated() || m_autoTerminated) &&
			(!IsStopped() || m_autoTerminated) &&
			m_hasParFiles)
		{
			bool forceRepair = (!m_password.Empty() || Util::EmptyStr(g_Options->GetUnpackPassFile()) || m_passListTried ||
				(!m_unpackDecryptError && !m_unpackPasswordError)) &&
				m_postInfo->GetNzbInfo()->GetParStatus() > NzbInfo::psSkipped;
			RequestParCheck(forceRepair);
		}
		else
#endif
		{
			PrintMessage(Message::mkError, "%s failed", *m_infoNameUp);
			m_postInfo->GetNzbInfo()->SetUnpackStatus(
				m_unpackSpaceError ? NzbInfo::usSpace :
				(m_unpackPasswordError || m_unpackDecryptError) ? NzbInfo::usPassword :
				NzbInfo::usFailure);
		}
	}
}

#ifndef DISABLE_PARCHECK
inline void UnpackController::RequestParCheck(bool forceRepair)
{
	PrintMessage(Message::mkInfo, "%s requested %s", *m_infoNameUp,
		forceRepair ? "par-check with forced repair" : "par-check/repair");
	m_postInfo->SetRequestParCheck(true);
	m_postInfo->SetForceRepair(forceRepair);
	m_postInfo->SetUnpackTried(true);
	m_postInfo->SetPassListTried(m_passListTried);
	m_postInfo->SetLastUnpackStatus(
		m_unpackSpaceError ? (int)NzbInfo::usSpace :
		(m_unpackPasswordError || m_unpackDecryptError) ? (int)NzbInfo::usPassword :
		(int)NzbInfo::usFailure);
}
#endif

inline void UnpackController::CreateUnpackDir()
{
	m_interDir = nzbget_rs_unpack_use_inter_dir(
		m_postInfo->GetNzbInfo()->GetDestDir(), g_Options->GetInterDir(), PATH_SEPARATOR) != 0;

	if (m_interDir && m_finalDir.Empty())
	{
		m_finalDir = m_postInfo->GetNzbInfo()->BuildFinalDirName();

		{
			GuardedDownloadQueue guard = DownloadQueue::Guard();
			m_postInfo->GetNzbInfo()->SetFinalDir(m_finalDir);
		}

		m_finalDirCreated = !FileSystem::DirectoryExists(m_finalDir);
	}

	const char* destDir = !m_finalDir.Empty() ? *m_finalDir : *m_destDir;

	if (g_Options->GetUseTempUnpackDir())
	{
		m_unpackDir.Format("%s%c%s", destDir, PATH_SEPARATOR, "_unpack");
	}
	else
	{
		m_unpackDir = destDir;
	}

	m_unpackDirCreated = !FileSystem::DirectoryExists(m_unpackDir);

	detail("Unpacking into %s", *m_unpackDir);

	CString errmsg;
	if (!FileSystem::ForceDirectories(m_unpackDir, errmsg))
	{
		PrintMessage(Message::mkError, "Could not create directory %s: %s", *m_unpackDir, *errmsg);
	}
}

inline void UnpackController::CheckArchiveFiles()
{
	std::vector<const char*> downloadedPtrs;
	downloadedPtrs.reserve(m_downloadedFiles.size());
	for (const CString& f : m_downloadedFiles)
	{
		downloadedPtrs.push_back(*f);
	}

	std::vector<const char*> extractedPtrs;
	if (m_postInfo && m_postInfo->GetExtractedArchives())
	{
		extractedPtrs.reserve(m_postInfo->GetExtractedArchives()->size());
		for (const CString& f : *m_postInfo->GetExtractedArchives())
		{
			extractedPtrs.push_back(*f);
		}
	}

	int hasRar = 0, hasNotUnpackedRar = 0, hasRenamed = 0;
	int hasSevenZip = 0, hasSevenZipMulti = 0, hasSplitted = 0;
	nzbget_rs_unpack_controller_check_archive_files(
		m_destDir,
		(g_Options && g_Options->GetUseTempUnpackDir()) ? 1 : 0,
		downloadedPtrs.data(),
		downloadedPtrs.size(),
		extractedPtrs.data(),
		extractedPtrs.size(),
		g_Options ? g_Options->GetUnpackIgnoreExt() : "",
		&hasRar,
		&hasNotUnpackedRar,
		&hasRenamed,
		&hasSevenZip,
		&hasSevenZipMulti,
		&hasSplitted);

	m_hasRarFiles = hasRar != 0;
	m_hasNotUnpackedRarFiles = hasNotUnpackedRar != 0;
	m_hasRenamedArchiveFiles = hasRenamed != 0;
	m_hasSevenZipFiles = hasSevenZip != 0;
	m_hasSevenZipMultiFiles = hasSevenZipMulti != 0;
	m_hasSplittedFiles = hasSplitted != 0;
}

inline bool UnpackController::Cleanup()
{
	std::vector<const char*> downloadedPtrs;
	downloadedPtrs.reserve(m_downloadedFiles.size());
	for (const CString& f : m_downloadedFiles)
	{
		downloadedPtrs.push_back(*f);
	}

	std::vector<const char*> joinedPtrs;
	joinedPtrs.reserve(m_joinedFiles.size());
	for (const CString& f : m_joinedFiles)
	{
		joinedPtrs.push_back(*f);
	}

	auto logCb = [](void* ctx, int kind, const char* msg)
	{
		if (!ctx || !msg) return;
		UnpackController* self = static_cast<UnpackController*>(ctx);
		Message::EKind msgKind = Message::mkInfo;
		switch (kind)
		{
			case 0: msgKind = Message::mkInfo; break;
			case 1: msgKind = Message::mkError; break;
			case 2: msgKind = Message::mkWarning; break;
			default: msgKind = Message::mkDetail; break;
		}
		self->PrintMessage(msgKind, "%s", msg);
	};

	int cleanedUpDisk = m_cleanedUpDisk ? 1 : 0;
	bool ok = nzbget_rs_unpack_controller_cleanup(
		m_destDir,
		m_finalDir,
		m_unpackDir,
		m_unpackOk ? 1 : 0,
		(g_Options && g_Options->GetUseTempUnpackDir()) ? 1 : 0,
		m_finalDirCreated ? 1 : 0,
		(g_Options && g_Options->GetUnpackCleanupDisk()) ? 1 : 0,
		m_interDir ? 1 : 0,
		m_hasSplittedFiles ? 1 : 0,
		static_cast<uint32_t>(FileSystem::uMask),
		downloadedPtrs.data(),
		downloadedPtrs.size(),
		joinedPtrs.data(),
		joinedPtrs.size(),
		&cleanedUpDisk,
		this,
		logCb) != 0;
	m_cleanedUpDisk = cleanedUpDisk != 0;
	return ok;
}

inline void UnpackController::AddMessage(Message::EKind kind, const char* text)
{
	BString<1024> msgText = text;
	NzbgetRsUnpackMessageInfo info{};
	nzbget_rs_unpack_parse_message(
		text, m_unpackExtendedDir, m_unpacker == upSevenZip ? 1 : 0, &info);
	if (m_unpacker == upUnrar && info.strip_extracting_dir != 0)
	{
		msgText.Format("Unrar: Extracting %s", text + info.stripped_suffix_offset);
	}

	if (m_postInfo)
	{
		m_postInfo->GetNzbInfo()->AddMessage(kind, msgText);
	}

	if (m_postInfo && m_unpacker == upUnrar && info.reset_stage_time != 0)
	{
		m_postInfo->SetStageTime(Util::CurrentTime());
	}

	if (m_postInfo && m_unpacker == upUnrar && info.has_extracting_progress != 0)
	{
		SetProgressLabel(msgText + 7);
	}

	if (m_postInfo && m_unpacker == upUnrar && info.has_extracting_from != 0)
	{
#ifdef DEBUG
		const char *filename = text + 23;
		debug("Filename: %s", filename);
#endif
		SetProgressLabel(text + 7);
	}

	if (info.decrypt_error != 0)
	{
		m_unpackDecryptError = true;
	}

	if (info.password_error != 0)
	{
		m_unpackPasswordError = true;
	}

	if (kind == Message::mkError &&
		(strstr(text, "No space left on device") ||
		 strstr(text, "Disk quota exceeded") ||
		 strstr(text, "os error 28") ||
		 strstr(text, "StorageFull")))
	{
		m_unpackSpaceError = true;
	}

	if (!IsStopped() && (m_unpackDecryptError || m_unpackPasswordError || info.crc_or_volume_error))
	{
		if (m_postInfo && !WillRetryWithPass())
		{
			m_postInfo->GetNzbInfo()->AddMessage(
				Message::mkWarning,
				BString<1024>("Cancelling %s due to errors", *m_infoName));
		}

		m_autoTerminated = true;
		Stop();
	}

	if (info.all_ok != 0)
	{
		m_allOkMessageReceived = true;
	}
}

inline void UnpackController::Stop()
{
	debug("Stopping unpack");
	Thread::Stop();
	bool isExecThread = std::this_thread::get_id() == m_execThreadId.load();
	if (m_unpacking && !isExecThread && !m_autoTerminated && HasActiveProcess())
	{
		if (Break())
		{
			for (int i = 0; i < 50 && m_unpacking && HasActiveProcess(); ++i)
			{
				Util::Sleep(100);
			}
		}
		if (m_unpacking)
		{
			Terminate();
		}
	}
	else
	{
		Terminate();
	}
}

inline void UnpackController::SetProgressLabel(const char* progressLabel)
{
	GuardedDownloadQueue guard = DownloadQueue::Guard();
	m_postInfo->SetProgressLabel(progressLabel);
}

inline bool UnpackController::HasCompletedArchiveFiles(NzbInfo* nzbInfo)
{
	for (CompletedFile& completedFile: nzbInfo->GetCompletedFiles())
	{
		if (nzbget_rs_unpack_has_completed_archive_ext(completedFile.GetFilename()) != 0)
		{
			return true;
		}
	}
	return false;
}

#endif
