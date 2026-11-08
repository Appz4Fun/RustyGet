/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2017 Andrey Prygunkov <hugbug@users.sourceforge.net>
 *  Copyright (C) 2024-2025 Denis <denis@nzbget.com>
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


#ifndef DIRECTUNPACK_H
#define DIRECTUNPACK_H

#include <atomic>
#include "Log.h"
#include "DownloadInfo.h"
#include "ScriptController.h"

class DirectUnpack final : public Thread, public ScriptController
{
public:
	virtual void Run();
	static void StartJob(NzbInfo* nzbInfo);
	void Stop(DownloadQueue* downloadQueue, NzbInfo* nzbInfo);
	void FileDownloaded(DownloadQueue* downloadQueue, FileInfo* fileInfo);
	void NzbDownloaded(DownloadQueue* downloadQueue, NzbInfo* nzbInfo);
	void NzbDeleted(DownloadQueue* downloadQueue, NzbInfo* nzbInfo);
	static bool IsArchiveFilename(const char* filename);
	bool IsUnpacking() const { return m_unpacking; }
	bool IsWaitingVolume() const { return m_waitingVolume; }

protected:
	virtual void AddMessage(Message::EKind kind, const char* text);

private:
	/**
	 *  @brief Checks if Direct unpack can proceed.
	 *
	 *  @return True if direct unpack is permitted (either the file is not an
 	 *          archive or the archive contains no failed articles), false otherwise.
	 */
	bool CanProceed(FileInfo* fileInfo) const;

	typedef std::deque<CString> ArchiveList;

	int m_nzbId;
	CString m_name;
	CString m_infoName;
	CString m_infoNameUp;
	CString m_destDir;
	CString m_finalDir;
	CString m_unpackDir;
	CString m_unpackExtendedDir;
	CString m_password;
	CString m_waitingFile;
	CString m_progressLabel;
	std::atomic<bool> m_nzbCompleted{false};
	Mutex m_volumeMutex;
	ArchiveList m_archives;
	time_t m_extraStartTime = 0;
	ArchiveList m_extractedArchives;
	bool m_allOkMessageReceived = false;
	bool m_unpackOk = false;
	bool m_finalDirCreated = false;
	bool m_processed = false;
	std::atomic<bool> m_cancelled{false};
	std::atomic<bool> m_waitingVolume{false};
	std::atomic<bool> m_unpacking{false};
	std::atomic<std::thread::id> m_execThreadId{};
	void CreateUnpackDir();
	void FindArchiveFiles();
	void ExecuteUnrar(const char* archiveName);
	void WaitNextVolume(const char* filename);
	void Cleanup();
	bool IsMainArchive(const char* filename);
	void SetProgressLabel(NzbInfo* nzbInfo, const char* progressLabel);
	void AddExtraTime(NzbInfo* nzbInfo);
};

#include <iterator>
#include "Util.h"
#include "FileSystem.h"
#include "Options.h"
#include "UnpackController.h"
#include "PrePostProcessor.h"
#include "nzbget_rs.h"

inline void DirectUnpack::StartJob(NzbInfo* nzbInfo)
{
	DirectUnpack* directUnpack = new DirectUnpack();
	directUnpack->m_nzbId = nzbInfo->GetId();
	directUnpack->m_unpacking = false;
	directUnpack->m_waitingVolume = false;
	directUnpack->SetAutoDestroy(true);

	nzbInfo->SetUnpackThread(directUnpack);
	nzbInfo->SetDirectUnpackStatus(NzbInfo::nsRunning);

	directUnpack->Start();
}

inline void DirectUnpack::Run()
{
	debug("Entering DirectUnpack-loop for %i", m_nzbId);

	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();

		NzbInfo* nzbInfo = downloadQueue->GetQueue()->Find(m_nzbId);
		if (!nzbInfo)
		{
			debug("Could not find NzbInfo for %i", m_nzbId);
			m_unpacking = false;
			return;
		}

		m_name = nzbInfo->GetName();
		m_destDir = nzbInfo->GetDestDir();
		m_finalDir = nzbInfo->BuildFinalDirName();

		NzbParameter* parameter = nzbInfo->GetParameters()->Find("*Unpack:Password");
		if (parameter)
		{
			m_password = parameter->GetValue();
		}
	}

	m_infoName.Format("direct unpack for %s", *m_name);
	m_infoNameUp.Format("Direct unpack for %s", *m_name);

	FindArchiveFiles();

	SetInfoName(m_infoName);
	SetWorkingDir(m_destDir);

	while (!IsStopped())
	{
		CString archive;
		{
			Guard guard(m_volumeMutex);
			if (!m_archives.empty())
			{
				archive = std::move(m_archives.front());
				m_archives.pop_front();
			}
		}

		if (archive)
		{
			m_waitingVolume = false;
			if (!m_processed)
			{
				PrintMessage(Message::mkInfo, "Directly unpacking %s", *m_name);
				Cleanup();
				CreateUnpackDir();
				m_processed = true;
			}

			size_t committedArchives = m_extractedArchives.size();
			ExecuteUnrar(archive);

			if (!m_unpackOk)
			{
				m_extractedArchives.resize(committedArchives);
				break;
			}
		}
		else
		{
			if (m_nzbCompleted)
			{
				break;
			}
			m_waitingVolume = true;
			Util::Sleep(100);
		}
	}

	m_unpacking = false;
	m_waitingVolume = false;

	bool interrupted = !m_cancelled && (IsStopped() || GetTerminated());
	if ((!m_unpackOk && !interrupted) || m_cancelled)
	{
		Cleanup();
	}

	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();

		NzbInfo* nzbInfo = downloadQueue->GetQueue()->Find(m_nzbId);
		if (!nzbInfo || nzbInfo->GetUnpackThread() != this)
		{
			debug("Could not find NzbInfo for %s", *m_infoName);
			return;
		}

		nzbInfo->SetUnpackThread(nullptr);
		nzbInfo->SetDirectUnpackStatus(
			!m_processed ? NzbInfo::nsNone :
			m_unpackOk && !interrupted && !m_cancelled ? NzbInfo::nsSuccess :
			interrupted ? NzbInfo::nsNone : NzbInfo::nsFailure);

		if (interrupted)
		{
			debug("%s stopped due to reload/shutdown, preserving unpack state", *m_infoName);
		}
		else if (nzbInfo->GetDirectUnpackStatus() == NzbInfo::nsSuccess ||
			nzbInfo->GetDirectUnpackStatus() == NzbInfo::nsFailure)
		{
			bool ok = nzbInfo->GetDirectUnpackStatus() == NzbInfo::nsSuccess;
			nzbInfo->AddMessage(ok ? Message::mkInfo : Message::mkWarning,
				BString<1024>("%s %s", *m_infoNameUp, ok ? "successful" : "failed"));
		}

		if (nzbInfo->GetExtractedArchives() &&
			(nzbInfo->GetDirectUnpackStatus() == NzbInfo::nsSuccess || interrupted))
		{
			std::move(m_extractedArchives.begin(), m_extractedArchives.end(),
				std::back_inserter(*nzbInfo->GetExtractedArchives()));
		}

		AddExtraTime(nzbInfo);

		if (nzbInfo->GetPostInfo() && !nzbInfo->GetLiveRepairThread())
		{
			nzbInfo->GetPostInfo()->SetWorking(false);
		}
	}

	debug("Exiting DirectUnpack-loop for %i", m_nzbId);
}

inline void DirectUnpack::ExecuteUnrar(const char* archiveName)
{
	m_unpackExtendedDir = FileSystem::MakeExtendedPath(m_unpackDir, true);
	m_allOkMessageReceived = false;

	Guard unpackGuard(UnpackController::GetUnpackMutex());
	if (IsStopped())
	{
		return;
	}
	m_waitingVolume = false;
	m_execThreadId = std::this_thread::get_id();
	m_unpacking = true;

	BString<1024> fullArchivePath("%s%c%s", *m_destDir, PATH_SEPARATOR, archiveName);
	auto logCb = [](void* ctx, int kind, const char* msg)
	{
		if (!ctx || !msg) return;
		DirectUnpack* self = static_cast<DirectUnpack*>(ctx);
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
		while (self->m_waitingVolume && !self->IsStopped() && !self->GetTerminated())
		{
			Util::Sleep(20);
		}
	};

	auto isStoppedCb = [](void* ctx) -> int
	{
		if (!ctx) return 0;
		DirectUnpack* self = static_cast<DirectUnpack*>(ctx);
		return (self->IsStopped() || self->GetTerminated()) ? 1 : 0;
	};

	const char* pw = m_password.Empty() ? "" : *m_password;
	const int cleanupDisk = (g_Options && g_Options->GetUnpackCleanupDisk()) ? 1 : 0;
	int ok = nzbget_rs_unpack_extract_with_zipfast(
		fullArchivePath,
		m_unpackDir,
		pw,
		1,
		cleanupDisk,
		logCb,
		this,
		isStoppedCb);

	m_unpacking = false;
	m_execThreadId = std::thread::id{};
	m_waitingVolume = false;

	m_unpackOk = ok != 0 && m_allOkMessageReceived && !GetTerminated() && !IsStopped();
}

inline void DirectUnpack::CreateUnpackDir()
{
	bool useInterDir = nzbget_rs_unpack_use_inter_dir(
		m_destDir, g_Options->GetInterDir(), PATH_SEPARATOR) != 0;

	m_finalDirCreated = useInterDir && !FileSystem::DirectoryExists(m_finalDir);

	const char* destDir = useInterDir && !m_finalDir.Empty() ? *m_finalDir : *m_destDir;

	if (g_Options->GetUseTempUnpackDir())
	{
		m_unpackDir.Format("%s%c%s", destDir, PATH_SEPARATOR, "_unpack");
	}
	else
	{
		m_unpackDir = destDir;
	}

	detail("Unpacking into %s", *m_unpackDir);

	CString errmsg;
	if (!FileSystem::ForceDirectories(m_unpackDir, errmsg))
	{
		PrintMessage(Message::mkError, "Could not create directory %s: %s", *m_unpackDir, *errmsg);
	}
}

inline void DirectUnpack::FindArchiveFiles()
{
	Guard guard(m_volumeMutex);
	auto addArchiveCb = [](void* ctx, const char* archiveName)
	{
		if (!ctx || !archiveName) return;
		static_cast<DirectUnpack*>(ctx)->m_archives.emplace_back(archiveName);
	};
	nzbget_rs_direct_unpack_find_archive_files(m_destDir, this, addArchiveCb);
}

inline bool DirectUnpack::IsMainArchive(const char* filename)
{
	return nzbget_rs_direct_unpack_is_main_archive(filename) != 0;
}

inline void DirectUnpack::AddMessage(Message::EKind kind, const char* text)
{
	debug("%s", text);

	BString<1024> msgText = text;
	NzbgetRsUnpackMessageInfo info{};
	nzbget_rs_unpack_parse_message(text, m_unpackExtendedDir, 0, &info);
	if (info.strip_extracting_dir != 0)
	{
		msgText.Format("Unrar: Extracting %s", text + info.stripped_suffix_offset);
	}
	if (info.is_insert_disk != 0)
	{
		BString<1024> filename;
		if (info.insert_disk_name_len > 0)
		{
			filename.Set(text + info.insert_disk_name_offset, (int)info.insert_disk_name_len);
		}
		WaitNextVolume(filename);
		return;
	}

	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
	NzbInfo* nzbInfo = downloadQueue->GetQueue()->Find(m_nzbId);
	if (nzbInfo)
	{
		nzbInfo->AddMessage(kind, msgText);
	}
	else
	{
		ScriptController::AddMessage(kind, msgText);
	}

	if (info.has_extracting_progress != 0 && nzbInfo)
	{
		SetProgressLabel(nzbInfo, msgText + 7);
	}

	if (info.has_extracting_from != 0 && nzbInfo)
	{
		SetProgressLabel(nzbInfo, text + 7);
		m_extractedArchives.emplace_back(text + 23);
	}

	if (!IsStopped() && (info.decrypt_error || info.direct_password_error || info.crc_or_volume_error))
	{
		Stop(downloadQueue, nzbInfo);
	}

	if (info.all_ok != 0)
	{
		m_allOkMessageReceived = true;
	}
}

inline void DirectUnpack::Stop(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
{
	debug("Stopping %s", *m_infoName);
	if (!(g_PrePostProcessor && g_PrePostProcessor->IsStopped()))
	{
		m_cancelled = true;
	}
	if (m_processed)
	{
		if (nzbInfo)
		{
			nzbInfo->AddMessage(Message::mkWarning, BString<1024>("Cancelling %s", *m_infoName));
		}
		else
		{
			warn("Cancelling %s", *m_infoName);
		}
	}
	if (nzbInfo)
	{
		AddExtraTime(nzbInfo);
		if (nzbInfo->GetPostInfo() && !nzbInfo->GetLiveRepairThread())
		{
			nzbInfo->GetPostInfo()->SetWorking(false);
		}
	}
	Thread::Stop();
	bool isExecThread = std::this_thread::get_id() == m_execThreadId.load();
	if (m_unpacking)
	{
		if (!isExecThread && !m_cancelled && HasActiveProcess() && Break())
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
}

inline void DirectUnpack::WaitNextVolume(const char* filename)
{
	debug("WaitNextVolume for %s", filename);

	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
		NzbInfo* nzbInfo = downloadQueue->GetQueue()->Find(m_nzbId);
		if (nzbInfo && (strcmp(m_destDir, nzbInfo->GetDestDir()) ||
			strcmp(m_finalDir, nzbInfo->BuildFinalDirName())))
		{
			nzbInfo->AddMessage(Message::mkWarning,
				BString<1024>("Stopping direct unpack for %s due to changed destination directory", nzbInfo->GetName()));
			Stop(downloadQueue, nzbInfo);
		}
	}

	BString<1024> fullFilename("%s%c%s", *m_destDir, PATH_SEPARATOR, filename);
	if (FileSystem::FileExists(fullFilename))
	{
		m_waitingVolume = false;
	}
	else
	{
		bool nzbCompleted;
		{
			Guard guard(m_volumeMutex);
			m_waitingFile = filename;
			m_waitingVolume = true;
			nzbCompleted = m_nzbCompleted;
		}
		if (nzbCompleted)
		{
			PrintMessage(Message::mkWarning, "Could not find volume %s", filename);
			GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
			NzbInfo* nzbInfo = downloadQueue->GetQueue()->Find(m_nzbId);
			Stop(downloadQueue, nzbInfo);
		}
	}
}

inline void DirectUnpack::FileDownloaded(DownloadQueue* downloadQueue, FileInfo* fileInfo)
{
	debug("FileDownloaded for %s/%s", fileInfo->GetNzbInfo()->GetName(), fileInfo->GetFilename());

	if (!CanProceed(fileInfo))
	{
		Stop(downloadQueue, fileInfo->GetNzbInfo());
		return;
	}

	Guard guard(m_volumeMutex);
	if (m_waitingFile && !strcasecmp(m_waitingFile, fileInfo->GetFilename()))
	{
		m_waitingFile = nullptr;
		m_waitingVolume = false;
	}

	if (IsMainArchive(fileInfo->GetFilename()))
	{
		m_archives.emplace_back(fileInfo->GetFilename());
	}
}

inline bool DirectUnpack::CanProceed(FileInfo* fileInfo) const
{
	return !IsArchiveFilename(fileInfo->GetFilename()) || fileInfo->GetFailedArticles() == 0;
}

inline void DirectUnpack::NzbDownloaded(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
{
	debug("NzbDownloaded for %s", nzbInfo->GetName());

	Guard guard(m_volumeMutex);
	m_nzbCompleted = true;
	if (m_waitingFile)
	{
		nzbInfo->AddMessage(Message::mkWarning,
			BString<1024>("Could not find volume %s", *m_waitingFile));
		Stop(downloadQueue, nzbInfo);
		return;
	}

	m_extraStartTime = Util::CurrentTime();

	if (nzbInfo->GetPostInfo())
	{
		nzbInfo->GetPostInfo()->SetProgressLabel(m_progressLabel);
	}
}

inline void DirectUnpack::NzbDeleted(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
{
	debug("NzbDeleted for %s", nzbInfo->GetName());

	nzbInfo->SetUnpackThread(nullptr);
	nzbInfo->SetDirectUnpackStatus(NzbInfo::nsFailure);
	Stop(downloadQueue, nzbInfo);
}

inline void DirectUnpack::Cleanup()
{
	debug("Cleanup for %s", *m_infoName);

	auto logCb = [](void* ctx, int kind, const char* msg)
	{
		if (!ctx || !msg) return;
		DirectUnpack* self = static_cast<DirectUnpack*>(ctx);
		self->PrintMessage(kind == 1 ? Message::mkError : Message::mkInfo, "%s", msg);
	};

	nzbget_rs_direct_unpack_cleanup(
		(g_Options && g_Options->GetUseTempUnpackDir()) ? 1 : 0,
		m_unpackDir,
		m_finalDirCreated ? 1 : 0,
		m_finalDir,
		this,
		logCb);
}

inline void DirectUnpack::SetProgressLabel(NzbInfo* nzbInfo, const char* progressLabel)
{
	m_progressLabel = progressLabel;
	if (nzbInfo->GetPostInfo())
	{
		nzbInfo->GetPostInfo()->SetProgressLabel(progressLabel);
	}
}

inline void DirectUnpack::AddExtraTime(NzbInfo* nzbInfo)
{
	if (m_extraStartTime)
	{
		int extraSec = (int)(Util::CurrentTime() - m_extraStartTime);
		nzbInfo->SetUnpackSec(nzbInfo->GetUnpackSec() + extraSec);
		nzbInfo->SetPostTotalSec(nzbInfo->GetPostTotalSec() + extraSec);
		m_extraStartTime = 0;
	}
}

inline bool DirectUnpack::IsArchiveFilename(const char* filename)
{
	if (!filename)
	{
		return false;
	}

	return nzbget_rs_file_type(22, filename, strlen(filename)) != 0;
}

#endif
