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


#include "nzbget.h"
#include "DirectUnpack.h"
#include "Log.h"
#include "Util.h"
#include "FileSystem.h"
#include "FileTypes.h"
#include "Options.h"
#include "UnpackController.h"
#include "PrePostProcessor.h"
#include "nzbget_rs.h"

void DirectUnpack::StartJob(NzbInfo* nzbInfo)
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

void DirectUnpack::Run()
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

		// the working flag is shared with a live stream-repair pass (option
		// <DupeArticleFallback> value "live"): while that thread is attached
		// it must keep holding the post-processing job, so it clears the flag
		// itself when it detaches
		if (nzbInfo->GetPostInfo() && !nzbInfo->GetLiveRepairThread())
		{
			nzbInfo->GetPostInfo()->SetWorking(false);
		}
	}

	debug("Exiting DirectUnpack-loop for %i", m_nzbId);
}

void DirectUnpack::ExecuteUnrar(const char* archiveName)
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

	const char* pw = m_password.Empty() ? "" : *m_password;
	int ok = nzbget_rs_unpack_extract_with_zipfast(
		fullArchivePath,
		m_unpackDir,
		pw,
		1,
		logCb,
		this);

	m_unpacking = false;
	m_execThreadId = std::thread::id{};
	m_waitingVolume = false;

	m_unpackOk = ok != 0 && m_allOkMessageReceived && !GetTerminated() && !IsStopped();
}

bool DirectUnpack::PrepareCmdParams(const char* command, ParamList* params, const char* infoName)
{
	if (FileSystem::FileExists(command))
	{
		params->emplace_back(command);
		return true;
	}

	std::vector<CString> cmdArgs = Util::SplitCommandLine(command);
	if (cmdArgs.empty())
	{
		PrintMessage(Message::mkError, "Could not start %s, failed to parse command line: %s", infoName, command);
		m_unpackOk = false;
		return false;
	}

	std::move(cmdArgs.begin(), cmdArgs.end(), std::back_inserter(*params));

	return true;
}

void DirectUnpack::CreateUnpackDir()
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

void DirectUnpack::FindArchiveFiles()
{
	Guard guard(m_volumeMutex);

	DirBrowser dir(m_destDir);
	while (const char* filename = dir.Next())
	{
		if (IsMainArchive(filename))
		{
			BString<1024> fullFilename("%s%c%s", *m_destDir, PATH_SEPARATOR, filename);
			if (!FileSystem::DirectoryExists(fullFilename))
			{
				m_archives.emplace_back(filename);
			}
		}
	}
}

bool DirectUnpack::IsMainArchive(const char* filename)
{
	if (!filename)
	{
		return false;
	}

	auto ext = FileSystem::GetFileExtension(filename);
	if (!ext || !FileTypes::IsRarExt(*ext))
	{
		return false;
	}

	return nzbget_rs_unpack_is_main_archive(filename) != 0;
}

/**
 * Unrar prints "Insert disk"-message without new line terminator.
 * In order to become the message we analyze the output after every char.
 */
bool DirectUnpack::ReadLine(char* buf, int bufSize, FILE* stream)
{
	int i = 0;
	int backspace = 0;

	for (; i < bufSize - 1; i++)
	{
		int ch = fgetc(stream);
		size_t outIdx = (size_t)i;
		if (nzbget_rs_unpack_direct_readline_step(
			reinterpret_cast<uint8_t*>(buf), (size_t)bufSize, (size_t)i, ch, &backspace, &outIdx) != 0)
		{
			i = (int)outIdx;
			break;
		}
	}

	// skip unnecessary progress messages
	if (!strncmp(buf, "...", 3))
	{
		buf[0] = '\0';
	}

	buf[i] = '\0';

	return i > 0;
}

void DirectUnpack::AddMessage(Message::EKind kind, const char* text)
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

void DirectUnpack::Stop(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
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
		// shared with a live stream-repair pass, which clears it itself (see
		// the Run() epilogue)
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

void DirectUnpack::WaitNextVolume(const char* filename)
{
	debug("WaitNextVolume for %s", filename);

	// Stop direct unpack if destination directory was changed during unpack
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
		Write("\n"); // emulating click on Enter-key for "continue"
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
		// the queue lock only after the volume lock is released: the queue side
		// takes them the other way round (a file completing takes the volume lock
		// under the queue lock), and the two threads could wait on each other
		if (nzbCompleted)
		{
			// nzb completed but unrar waits for another volume
			PrintMessage(Message::mkWarning, "Could not find volume %s", filename);
			GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
			NzbInfo* nzbInfo = downloadQueue->GetQueue()->Find(m_nzbId);
			Stop(downloadQueue, nzbInfo);
		}
	}
}

void DirectUnpack::FileDownloaded(DownloadQueue* downloadQueue, FileInfo* fileInfo)
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
		Write("\n"); // emulating click on Enter-key for "continue"
	}

	if (IsMainArchive(fileInfo->GetFilename()))
	{
		m_archives.emplace_back(fileInfo->GetFilename());
	}
}

bool DirectUnpack::CanProceed(FileInfo* fileInfo) const
{
	return !IsArchiveFilename(fileInfo->GetFilename()) || fileInfo->GetFailedArticles() == 0;
}

void DirectUnpack::NzbDownloaded(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
{
	debug("NzbDownloaded for %s", nzbInfo->GetName());

	Guard guard(m_volumeMutex);
	m_nzbCompleted = true;
	if (m_waitingFile)
	{
		// nzb completed but unrar waits for another volume
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

void DirectUnpack::NzbDeleted(DownloadQueue* downloadQueue, NzbInfo* nzbInfo)
{
	debug("NzbDeleted for %s", nzbInfo->GetName());

	nzbInfo->SetUnpackThread(nullptr);
	nzbInfo->SetDirectUnpackStatus(NzbInfo::nsFailure);
	Stop(downloadQueue, nzbInfo);
}

// Remove _unpack-dir
void DirectUnpack::Cleanup()
{
	debug("Cleanup for %s", *m_infoName);

	// only a temporary unpack folder: without one (UseTempUnpackDir=no) it is the
	// download's own folder (or the final one), and deleting it removed every file
	// downloaded so far
	CString errmsg;
	if (g_Options->GetUseTempUnpackDir() && FileSystem::DirectoryExists(m_unpackDir) &&
		!FileSystem::DeleteDirectoryWithContent(m_unpackDir, errmsg))
	{
		PrintMessage(Message::mkError, "Could not delete temporary directory %s: %s", *m_unpackDir, *errmsg);
	}

	if (m_finalDirCreated)
	{
		FileSystem::DeleteDirectory(m_finalDir);
	}
}

void DirectUnpack::SetProgressLabel(NzbInfo* nzbInfo, const char* progressLabel)
{
	m_progressLabel = progressLabel;
	if (nzbInfo->GetPostInfo())
	{
		nzbInfo->GetPostInfo()->SetProgressLabel(progressLabel);
	}
}

void DirectUnpack::AddExtraTime(NzbInfo* nzbInfo)
{
	if (m_extraStartTime)
	{
		int extraSec = (int)(Util::CurrentTime() - m_extraStartTime);
		nzbInfo->SetUnpackSec(nzbInfo->GetUnpackSec() + extraSec);
		nzbInfo->SetPostTotalSec(nzbInfo->GetPostTotalSec() + extraSec);
		m_extraStartTime = 0;
	}
}

bool DirectUnpack::IsArchiveFilename(const char* filename)
{
	if (!filename)
	{
		return false;
	}

	return FileTypes::IsRarFile(filename);
}
