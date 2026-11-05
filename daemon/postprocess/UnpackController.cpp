/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2013-2018 Andrey Prygunkov <hugbug@users.sourceforge.net>
 *  Copyright (C) 2023-2026 Denis <denis@nzbget.com>
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
#include "UnpackController.h"
#include "Log.h"
#include "Util.h"
#include "FileSystem.h"
#include "ParParser.h"
#include "Options.h"
#include "nzbget_rs.h"

UnpackController::~UnpackController()
{
#ifndef DISABLE_TLS
	OpenSSL::StopSSLThread();
#endif
}

bool UnpackController::FileList::Exists(const char* filename)
{
	return std::find(begin(), end(), filename) != end();
}

bool UnpackController::ParamList::Exists(const char* param)
{
	return std::find(begin(), end(), param) != end();
}

static Mutex s_unpackMutex;

Mutex& UnpackController::GetUnpackMutex()
{
	return s_unpackMutex;
}

void UnpackController::StartJob(PostInfo* postInfo)
{
	UnpackController* unpackController = new UnpackController();
	unpackController->m_postInfo = postInfo;
	unpackController->SetAutoDestroy(false);

	postInfo->SetPostThread(unpackController);

	unpackController->Start();
}

void UnpackController::Run()
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

void UnpackController::UnpackArchives(EUnpacker unpacker, bool multiVolumes)
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

void UnpackController::ExecuteUnpack(EUnpacker unpacker, const char* password, bool multiVolumes)
{
	switch (unpacker)
	{
		case upUnrar:
			ExecuteUnrar(password);
			break;

		case upSevenZip:
			ExecuteSevenZip(password, multiVolumes);
			break;
	}
}

void UnpackController::ExecuteUnrar(const char* password)
{
	// Format:
	//   unrar x -y -p- -o+ *.rar ./_unpack/

	ParamList params;
	if (!PrepareCmdParams(g_Options->GetUnrarCmd(), &params, "unrar"))
	{
		return;
	}

	if (!params.Exists("x") && !params.Exists("e"))
	{
		params.emplace_back("x");
	}

	params.emplace_back("-y");

	if (!Util::EmptyStr(password))
	{
		params.emplace_back(BString<1024>("-p%s", password));
	}
	else
	{
		params.emplace_back("-p-");
	}

	if (!params.Exists("-o+") && !params.Exists("-o-") && !params.Exists("-or"))
	{
		params.emplace_back("-o+");
	}

	params.emplace_back("*.rar");
	m_unpackExtendedDir = FileSystem::MakeExtendedPath(m_unpackDir, true);
	params.emplace_back(BString<1024>("%s%c", *m_unpackExtendedDir, PATH_SEPARATOR));
	SetArgs(std::move(params));
	SetLogPrefix("Unrar");
	ResetEnv();

	m_allOkMessageReceived = false;
	m_unpacker = upUnrar;

	SetProgressLabel("");
	Guard unpackGuard(GetUnpackMutex());
	m_execThreadId = std::this_thread::get_id();
	m_unpacking = true;
	int exitCode = Execute();
	m_unpacking = false;
	m_execThreadId = std::thread::id{};
	SetLogPrefix(nullptr);
	SetProgressLabel("");

	m_unpackOk = exitCode == 0 && m_allOkMessageReceived && !GetTerminated();
	m_unpackStartError = exitCode == -1 && !m_autoTerminated;
	m_unpackSpaceError = exitCode == 5;
	m_unpackPasswordError |= exitCode == 11;

	if (!m_unpackOk && exitCode > 0 && !WillRetryWithPass() && !IsStopped() && !GetTerminated())
	{
		PrintMessage(Message::mkError, "Unrar error code: %i", exitCode);
	}
}

void UnpackController::ExecuteSevenZip(const char* password, bool multiVolumes)
{
	// Format:
	//   7z x -y -p- -o./_unpack *.7z
	// OR
	//   7z x -y -p- -o./_unpack *.7z.001

	ParamList params;
	if (!PrepareCmdParams(g_Options->GetSevenZipCmd(), &params, "7-Zip"))
	{
		return;
	}

	if (!params.Exists("x") && !params.Exists("e"))
	{
		params.emplace_back("x");
	}

	params.emplace_back("-y");

	if (!Util::EmptyStr(password))
	{
		params.emplace_back(BString<1024>("-p%s", password));
	}
	else
	{
		params.emplace_back("-p-");
	}

	params.emplace_back(BString<1024>("-o%s", *m_unpackDir));
	params.emplace_back(multiVolumes ? "*.7z.001" : "*.7z");
	SetArgs(std::move(params));
	ResetEnv();

	m_allOkMessageReceived = false;
	m_unpacker = upSevenZip;

	PrintMessage(Message::mkInfo, "Executing 7-Zip");
	SetLogPrefix("7-Zip");
	SetProgressLabel("");
	Guard unpackGuard(GetUnpackMutex());
	m_execThreadId = std::this_thread::get_id();
	m_unpacking = true;
	int exitCode = Execute();
	m_unpacking = false;
	m_execThreadId = std::thread::id{};
	SetLogPrefix(nullptr);
	SetProgressLabel("");

	m_unpackOk = exitCode == 0 && m_allOkMessageReceived && !GetTerminated();
	m_unpackStartError = exitCode == -1 && !m_autoTerminated;

	if (!m_unpackOk && exitCode > 0 && !WillRetryWithPass() && !IsStopped() && !GetTerminated())
	{
		PrintMessage(Message::mkError, "7-Zip error code: %i", exitCode);
	}
}

bool UnpackController::WillRetryWithPass()
{
	return (m_unpackDecryptError || m_unpackPasswordError) &&
		m_password.Empty() && !g_Options->GetUnpackPassFilePath().empty();
}

bool UnpackController::PrepareCmdParams(const char* command, ParamList* params, const char* infoName)
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
		m_unpackStartError = true;
		return false;
	}

	std::move(cmdArgs.begin(), cmdArgs.end(), std::back_inserter(*params));

	return true;
}

void UnpackController::JoinSplittedFiles()
{
	SetLogPrefix("Join");
	SetProgressLabel("");
	m_postInfo->SetStageProgress(0);

	// determine groups

	DirBrowser dir(m_destDir);
	while (const char* filename = dir.Next())
	{
		BString<1024> fullFilename("%s%c%s", *m_destDir, PATH_SEPARATOR, filename);

		if (!FileSystem::DirectoryExists(fullFilename) &&
			nzbget_rs_unpack_is_split_first_piece(filename) != 0 &&
			!FileHasRarSignature(fullFilename))
		{
			if (!JoinFile(filename))
			{
				m_unpackOk = false;
				break;
			}
		}
	}

	SetLogPrefix(nullptr);
	SetProgressLabel("");
}

bool UnpackController::JoinFile(const char* fragBaseName)
{
	BString<1024> destBaseName = fragBaseName;
	if (char* ext = strrchr(destBaseName, '.'))
	{
		*ext = '\0';
	}

	BString<1024> fullFilename("%s%c%s", *m_destDir, PATH_SEPARATOR, fragBaseName);
	int64 firstSegmentSize = FileSystem::FileSize(fullFilename);

	// Validate joinable file:
	//  - fragments have continuous numbers (no holes);
	//  - fragments have the same size (except of the last fragment);
	//  - the last fragment must be smaller than other fragments,
	//  if it has the same size it is probably not the last and there are missing fragments.

	int count = 0;
	int min = -1;
	int max = -1;
	int difSizeMin = 999999;
	DirBrowser dir(m_destDir);
	while (const char* filename = dir.Next())
	{
		fullFilename.Format("%s%c%s", *m_destDir, PATH_SEPARATOR, filename);

		// only this file's pieces (its name plus ".NNN"): with a second set of split
		// files in the folder, their pieces were counted and compared too, and the join
		// failed for "missing fragments"
		if (!FileSystem::DirectoryExists(fullFilename))
		{
			int64 segmentSize = FileSystem::FileSize(fullFilename);
			nzbget_rs_unpack_accumulate_split_piece(
				filename, destBaseName, segmentSize, firstSegmentSize,
				&count, &min, &max, &difSizeMin);
		}
	}

	if (nzbget_rs_unpack_check_joinable_fragments(
		count, min, max, difSizeMin,
		m_postInfo->GetNzbInfo()->GetParStatus() == NzbInfo::psSuccess ? 1 : 0) == 0)
	{
		PrintMessage(Message::mkWarning, "Could not join splitted file %s: missing fragments detected", *destBaseName);
		return false;
	}

	// Now can join
	PrintMessage(Message::mkInfo, "Joining splitted file %s", *destBaseName);
	m_postInfo->SetStageProgress(0);

	BString<1024> destFilename("%s%c%s", *m_unpackDir, PATH_SEPARATOR, *destBaseName);

	DiskFile outFile;
	if (!outFile.Open(destFilename, DiskFile::omWrite))
	{
		PrintMessage(Message::mkError, "Could not create file %s: %s", *destFilename,
			*FileSystem::GetLastErrorMessage());
		return false;
	}
	if (g_Options->GetWriteBuffer() > 0)
	{
		outFile.SetWriteBuffer(g_Options->GetWriteBuffer() * 1024);
	}

	// the pieces' total, to check the joined size against: first * (count - 1) +
	// the different one assumed a shorter last piece, and with all pieces of one
	// size (common) every join failed the check
	int64 totalSize = 0;
	for (int i = min; i <= max; i++)
	{
		BString<1024> fragFilename("%s%c%s.%.3i", *m_destDir, PATH_SEPARATOR, *destBaseName, i);
		if (FileSystem::FileExists(fragFilename))
		{
			totalSize += FileSystem::FileSize(fragFilename);
		}
	}
	if (totalSize <= 0)
	{
		PrintMessage(Message::mkError, "Could not join splitted file %s: file segments are empty", *destBaseName);
		return false;
	}
	int64 written = 0;

	CharBuffer buffer(1024 * 50);

	bool ok = true;
	for (int i = min; i <= max; i++)
	{
		CString pieceName = CString::FormatStr("%s.%.3i", *destBaseName, i);
		PrintMessage(Message::mkInfo, "Joining from %s", *pieceName);
		SetProgressLabel(BString<1024>("Joining from %s", *pieceName));

		BString<1024> fragFilename("%s%c%s.%.3i", *m_destDir, PATH_SEPARATOR, *destBaseName, i);
		if (!FileSystem::FileExists(fragFilename))
		{
			break;
		}

		DiskFile inFile;
		if (inFile.Open(fragFilename, DiskFile::omRead))
		{
			// every write checked: with the disk full the joined file was cut short,
			// reported a success, and the pieces were then deleted
			int cnt = buffer.Size();
			while (cnt == buffer.Size())
			{
				cnt = (int)inFile.Read(buffer, buffer.Size());
				if (cnt > 0 && outFile.Write(buffer, cnt) != cnt)
				{
					PrintMessage(Message::mkError, "Could not write file %s: %s", *destFilename,
						*FileSystem::GetLastErrorMessage());
					ok = false;
					break;
				}
				if (cnt > 0)
				{
					written += cnt;
				}
				m_postInfo->SetStageProgress(int(written * 1000 / totalSize));
			}
			inFile.Close();
			if (!ok)
			{
				break;
			}

			m_joinedFiles.push_back(std::move(pieceName));
		}
		else
		{
			PrintMessage(Message::mkError, "Could not open file %s", *fragFilename);
			ok = false;
			break;
		}
	}

	if (!outFile.Close() && ok)
	{
		PrintMessage(Message::mkError, "Could not write file %s: %s", *destFilename,
			*FileSystem::GetLastErrorMessage());
		ok = false;
	}
	if (ok && written != totalSize)
	{
		PrintMessage(Message::mkError, "Could not join splitted file %s: joined %lli of %lli bytes",
			*destBaseName, (long long)written, (long long)totalSize);
		ok = false;
	}

	return ok;
}

void UnpackController::Completed()
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
void UnpackController::RequestParCheck(bool forceRepair)
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

void UnpackController::CreateUnpackDir()
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

void UnpackController::CheckArchiveFiles()
{

	DirBrowser dir(m_destDir);
	while (const char* filename = dir.Next())
	{
		BString<1024> fullFilename("%s%c%s", *m_destDir, PATH_SEPARATOR, filename);

		if (!FileSystem::DirectoryExists(fullFilename) && IsDownloadedFile(filename))
		{
			const int archiveKind = nzbget_rs_unpack_classify_archive_file(filename);
			if (archiveKind == 1)
			{
				const auto begin = m_postInfo->GetExtractedArchives()->cbegin();
				const auto end = m_postInfo->GetExtractedArchives()->cend();
				m_hasRarFiles = true;
				m_hasNotUnpackedRarFiles |= std::find(begin, end, filename) == end;
			}
			else if (archiveKind == 2)
			{
				m_hasSevenZipFiles = true;
			}
			else if (archiveKind == 3)
			{
				m_hasSevenZipMultiFiles = true;
			}
			else if (archiveKind == 4)
			{
				m_hasSplittedFiles = true;
			}
			else if (!m_hasRenamedArchiveFiles && (archiveKind == 0 || archiveKind == 6) &&
				!Util::MatchFileExt(filename, g_Options->GetUnpackIgnoreExt(), ",;") &&
				FileHasRarSignature(fullFilename))
			{
				m_hasRenamedArchiveFiles = true;
			}
		}
	}
}

// Without a temporary unpack folder, extracted files land next to the downloaded
// ones (and an archive may hold archives): only the download's own files count as
// its archives. Without a record of them, every file does.
bool UnpackController::IsDownloadedFile(const char* filename)
{
	return g_Options->GetUseTempUnpackDir() || m_downloadedFiles.empty() ||
		m_downloadedFiles.Exists(filename);
}

bool UnpackController::FileHasRarSignature(const char* filename)
{
	uint8_t fileSignature[8]{};
	int cnt = 0;
	DiskFile infile;
	if (infile.Open(filename, DiskFile::omRead))
	{
		cnt = (int)infile.Read(fileSignature, sizeof(fileSignature));
		infile.Close();
	}
	return cnt == (int)sizeof(fileSignature) &&
		nzbget_rs_unpack_detect_rar_signature(fileSignature, sizeof(fileSignature)) != 0;
}

bool UnpackController::Cleanup()
{
	// By success:
	//   - move unpacked files to destination dir;
	//   - remove _unpack-dir;
	//   - delete archive-files.
	// By failure:
	//   - remove _unpack-dir.

	bool ok = true;

	FileList extractedFiles;

	if (m_unpackOk && g_Options->GetUseTempUnpackDir())
	{
		// moving files back
		DirBrowser dir(m_unpackDir);
		const char* destDir = !m_finalDir.Empty() ? *m_finalDir : *m_destDir;
		while (const char* filename = dir.Next())
		{
			BString<1024> srcFile("%s%c%s", *m_unpackDir, PATH_SEPARATOR, filename);
			BString<1024> dstFile("%s%c%s", destDir, PATH_SEPARATOR, *FileSystem::MakeValidFilename(filename));

			// silently overwrite existing files
			FileSystem::DeleteFile(dstFile);

			bool hiddenFile = filename[0] == '.';

			if (!FileSystem::MoveFile(srcFile, dstFile) && !hiddenFile)
			{
				PrintMessage(Message::mkError, "Could not move file %s to %s: %s", *srcFile, *dstFile,
					*FileSystem::GetLastErrorMessage());
				ok = false;
			}

			// Fixing file or directory permissions overridden by the unpacker
			FileSystem::RestoreFileOrDirPermissions(dstFile.Str());

			extractedFiles.push_back(filename);
		}

		// Fixing directory permissions overridden by the unpacker
		FileSystem::RestoreDirPermissions(destDir);

	}

	CString errmsg;
	if (ok && g_Options->GetUseTempUnpackDir() && !FileSystem::DeleteDirectoryWithContent(m_unpackDir, errmsg))
	{
		PrintMessage(Message::mkError, "Could not delete temporary directory %s: %s", *m_unpackDir, *errmsg);
	}

	if (!m_unpackOk && m_finalDirCreated)
	{
		FileSystem::DeleteDirectory(m_finalDir);
	}

	if (m_unpackOk && ok && g_Options->GetUnpackCleanupDisk())
	{
		PrintMessage(Message::mkInfo, "Deleting archive files");


		DirBrowser dir(m_destDir);
		while (const char* filename = dir.Next())
		{
			BString<1024> fullFilename("%s%c%s", *m_destDir, PATH_SEPARATOR, filename);

			int cleanupKind = nzbget_rs_unpack_classify_cleanup_archive_file(
				FileSystem::DirectoryExists(fullFilename) ? 1 : 0,
				m_interDir ? 1 : 0,
				extractedFiles.Exists(filename) ? 1 : 0,
				IsDownloadedFile(filename) ? 1 : 0,
				m_hasSplittedFiles ? 1 : 0,
				m_joinedFiles.Exists(filename) ? 1 : 0,
				filename);
			if (cleanupKind == 1 || (cleanupKind == 2 && FileHasRarSignature(fullFilename)))
			{
				PrintMessage(Message::mkInfo, "Deleting file %s", filename);

				if (!FileSystem::DeleteFile(fullFilename))
				{
					PrintMessage(Message::mkError, "Could not delete file %s: %s", *fullFilename,
						*FileSystem::GetLastErrorMessage());
				}
			}
		}

		m_cleanedUpDisk = true;
	}

	return ok;
}

/**
 * Unrar prints progress information into the same line using backspace control character.
 * In order to print progress continuously we analyze the output after every char
 * and update post-job progress information.
 */
bool UnpackController::ReadLine(char* buf, int bufSize, FILE* stream)
{
	if (bufSize <= 0)
	{
		return false;
	}
	int32_t printed = 0;
	size_t idx = 0;
	ptrdiff_t lastBs = -1;

	while (idx + 1 < (size_t)bufSize)
	{
		int ch = fgetc(stream);
		ptrdiff_t procLen = -1;
		int32_t progress = -1;
		int32_t shouldBreak = nzbget_rs_unpack_ctrl_readline_step(
			reinterpret_cast<uint8_t*>(buf), (size_t)bufSize, &idx, ch,
			&printed, &lastBs, &procLen, &progress);
		if (procLen >= 0)
		{
			BString<1024> tmp;
			if (procLen > 0)
			{
				tmp.Set(buf, (int)procLen);
			}
			ProcessOutput(tmp);
		}
		if (progress >= 0)
		{
			m_postInfo->SetStageProgress(progress);
		}
		if (shouldBreak != 0)
		{
			break;
		}
	}

	buf[idx] = '\0';

	if (printed != 0)
	{
		buf[0] = '\0';
	}

	return idx > 0;
}

void UnpackController::AddMessage(Message::EKind kind, const char* text)
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

	if (m_unpacker == upUnrar && info.password_error != 0)
	{
		m_unpackPasswordError = true;
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

void UnpackController::Stop()
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

void UnpackController::SetProgressLabel(const char* progressLabel)
{
	GuardedDownloadQueue guard = DownloadQueue::Guard();
	m_postInfo->SetProgressLabel(progressLabel);
}

bool UnpackController::HasCompletedArchiveFiles(NzbInfo* nzbInfo)
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
