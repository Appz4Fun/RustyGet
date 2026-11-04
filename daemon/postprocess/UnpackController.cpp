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
 *  along with this program.  If not, see <https://www.gnu.org/licenses/>.
 */


#include "nzbget.h"
#include "UnpackController.h"
#include "Log.h"
#include "Util.h"
#include "FileSystem.h"
#include "ParParser.h"
#include "Options.h"
#ifdef NZBGET_USE_RUST
#include "nzbget_rs.h"
#endif

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
#ifdef NZBGET_USE_RUST
		unpack = nzbget_rs_ucf_unpack_ctrl_eval_unpack_param(
			parameter ? parameter->GetValue() : nullptr) != 0;
#else
		unpack = !(parameter && !strcasecmp(parameter->GetValue(), "no"));
#endif

		parameter = m_postInfo->GetNzbInfo()->GetParameters()->Find("*Unpack:Password");
		if (parameter)
		{
			m_password = parameter->GetValue();
		}
	}

	m_infoName.Format("unpack for %s", *m_name);
	m_infoNameUp.Format("Unpack for %s", *m_name); // first letter in upper case

	m_hasParFiles = ParParser::FindMainPars(m_destDir, nullptr);

	if (unpack)
	{
		CheckArchiveFiles();
	}

	SetInfoName(m_infoName);
	SetWorkingDir(m_destDir);

#ifdef NZBGET_USE_RUST
	bool hasFiles = nzbget_rs_ucf_unpack_ctrl_has_any_archive_files(
		m_hasRarFiles ? 1 : 0, m_hasSevenZipFiles ? 1 : 0,
		m_hasSevenZipMultiFiles ? 1 : 0, m_hasSplittedFiles ? 1 : 0) != 0;
#else
	bool hasFiles = m_hasRarFiles || m_hasSevenZipFiles || m_hasSevenZipMultiFiles || m_hasSplittedFiles;
#endif

#ifdef NZBGET_USE_RUST
	if (nzbget_rs_dur_unpack_ctrl_should_skip_second_attempt(
		m_postInfo->GetUnpackTried() ? 1 : 0,
		m_postInfo->GetParRepaired() ? 1 : 0,
		m_password.Empty() ? 1 : 0,
		Util::EmptyStr(g_Options->GetUnpackPassFile()) ? 1 : 0,
		m_postInfo->GetPassListTried() ? 1 : 0) != 0)
#else
	if (m_postInfo->GetUnpackTried() && !m_postInfo->GetParRepaired() &&
		(!m_password.Empty() || Util::EmptyStr(g_Options->GetUnpackPassFile()) || m_postInfo->GetPassListTried()))
#endif
	{
		PrintMessage(Message::mkInfo, "Second unpack attempt skipped for %s due to par-check not repaired anything", *m_name);
		PrintMessage(Message::mkError,
			m_postInfo->GetLastUnpackStatus() == (int)NzbInfo::usPassword ?
				 "%s failed: checksum error in the encrypted file. Corrupt file or wrong password." : "%s failed.",
			*m_infoNameUp);
		m_postInfo->GetNzbInfo()->SetUnpackStatus((NzbInfo::EPostUnpackStatus)m_postInfo->GetLastUnpackStatus());
	}
	else
	{
#ifdef NZBGET_USE_RUST
#ifndef DISABLE_PARCHECK
		int hasParFilesFlag = m_hasParFiles ? 1 : 0;
#else
		int hasParFilesFlag = 0;
#endif
		int runBranch = nzbget_rs_pnd_unpack_ctrl_classify_run_branch(
			unpack ? 1 : 0, hasFiles ? 1 : 0,
			(int)m_postInfo->GetNzbInfo()->GetParStatus(), (int)NzbInfo::psSkipped,
			(int)m_postInfo->GetNzbInfo()->GetParRenameStatus(), (int)NzbInfo::rsSkipped,
			hasParFilesFlag, m_hasRenamedArchiveFiles ? 1 : 0);
		if (runBranch == 1)
		{
			PrintMessage(Message::mkInfo, "Unpacking %s", *m_name);

			CreateUnpackDir();

			if (m_hasRarFiles)
			{
				int rarLogKind = 0;
				bool runUnrar = nzbget_rs_pnd_unpack_ctrl_eval_rar_unpack_mode(
					m_hasNotUnpackedRarFiles ? 1 : 0, m_unpackDirCreated ? 1 : 0,
					(int)m_postInfo->GetNzbInfo()->GetDirectUnpackStatus(),
					(int)NzbInfo::nsSuccess, &rarLogKind) != 0;
				if (rarLogKind == 1)
				{
					PrintMessage(Message::mkWarning, "Could not find files unpacked by direct unpack, unpacking all files again");
				}
				else if (rarLogKind == 2)
				{
					PrintMessage(Message::mkInfo, "Found archive files not processed by direct unpack, unpacking all files again");
				}
				else if (rarLogKind == 3)
				{
					PrintMessage(Message::mkInfo, "Using directly unpacked files");
				}
				if (runUnrar)
				{
					// Discard info about extracted archives to prevent reusing on next unpack attempt
					m_postInfo->GetExtractedArchives()->clear();

					UnpackArchives(upUnrar, false);
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
#ifndef DISABLE_PARCHECK
		else if (runBranch == 2)
		{
			PrintMessage(Message::mkInfo, "Nothing to unpack for %s", *m_name);
			RequestParCheck(false);
		}
#endif
		else if (runBranch == 3)
		{
			PrintMessage(Message::mkError, "Could not unpack %s due to renamed archive files", *m_name);
			m_postInfo->GetNzbInfo()->SetUnpackStatus(NzbInfo::usFailure);
		}
		else if (runBranch == 4)
		{
			PrintMessage(Message::mkInfo, "Nothing to unpack for %s", *m_name);
			m_postInfo->GetNzbInfo()->SetUnpackStatus(NzbInfo::usSkipped);
		}
		else
		{
			PrintMessage(Message::mkInfo, "Unpack for %s skipped", *m_name);
			m_postInfo->GetNzbInfo()->SetUnpackStatus(NzbInfo::usSkipped);
		}
#else
		if (unpack && hasFiles)
		{
			PrintMessage(Message::mkInfo, "Unpacking %s", *m_name);

			CreateUnpackDir();

			if (m_hasRarFiles)
			{
				if (m_hasNotUnpackedRarFiles || m_unpackDirCreated)
				{
					if (m_postInfo->GetNzbInfo()->GetDirectUnpackStatus() == NzbInfo::nsSuccess)
					{
						if (m_unpackDirCreated)
						{
							PrintMessage(Message::mkWarning, "Could not find files unpacked by direct unpack, unpacking all files again");
						}
						else
						{
							PrintMessage(Message::mkInfo, "Found archive files not processed by direct unpack, unpacking all files again");
						}
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
#endif
	}


	int unpackSec = (int)(Util::CurrentTime() - start);
	m_postInfo->GetNzbInfo()->SetUnpackSec(m_postInfo->GetNzbInfo()->GetUnpackSec() + unpackSec);

	m_postInfo->SetWorking(false);
}

void UnpackController::UnpackArchives(EUnpacker unpacker, bool multiVolumes)
{
#ifdef NZBGET_USE_RUST
	if (nzbget_rs_pur_unpack_ctrl_should_execute_initial_unpack(
		m_postInfo->GetUnpackTried() ? 1 : 0,
		m_postInfo->GetParRepaired() ? 1 : 0) != 0)
#else
	if (!m_postInfo->GetUnpackTried() || m_postInfo->GetParRepaired())
#endif
	{
		ExecuteUnpack(unpacker, m_password, multiVolumes);
#ifdef NZBGET_USE_RUST
		if (nzbget_rs_pup_unpack_ctrl_should_try_par_before_pass_file(
			m_unpackOk ? 1 : 0, m_hasParFiles ? 1 : 0, m_unpackPasswordError ? 1 : 0,
			(int)m_postInfo->GetNzbInfo()->GetParStatus(), (int)NzbInfo::psSkipped) != 0)
#else
		if (!m_unpackOk && m_hasParFiles && !m_unpackPasswordError &&
			m_postInfo->GetNzbInfo()->GetParStatus() <= NzbInfo::psSkipped)
#endif
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

#ifdef NZBGET_USE_RUST
	if (nzbget_rs_pup_unpack_ctrl_should_try_password_file(
		m_unpackOk ? 1 : 0, m_unpackStartError ? 1 : 0, m_unpackSpaceError ? 1 : 0,
		m_unpackDecryptError ? 1 : 0, m_unpackPasswordError ? 1 : 0,
		GetTerminated() ? 1 : 0, m_autoTerminated ? 1 : 0,
		m_password.Empty() ? 1 : 0, Util::EmptyStr(g_Options->GetUnpackPassFile()) ? 1 : 0) != 0)
#else
	if (!m_unpackOk && !m_unpackStartError && !m_unpackSpaceError &&
		(m_unpackDecryptError || m_unpackPasswordError) &&
		(!GetTerminated() || m_autoTerminated) &&
		m_password.Empty() && !Util::EmptyStr(g_Options->GetUnpackPassFile()))
#endif
	{
		DiskFile infile;
		if (!infile.Open(g_Options->GetUnpackPassFile(), DiskFile::omRead))
		{
			PrintMessage(Message::mkError, "Could not open file %s", g_Options->GetUnpackPassFile());
			return;
		}

		char password[512];
#ifdef NZBGET_USE_RUST
		while (nzbget_rs_ucf_unpack_ctrl_can_continue_password_loop(
			m_unpackOk ? 1 : 0, m_unpackStartError ? 1 : 0, m_unpackSpaceError ? 1 : 0,
			m_unpackDecryptError ? 1 : 0, m_unpackPasswordError ? 1 : 0) != 0 &&
			infile.ReadLine(password, sizeof(password) - 1))
#else
		while (!m_unpackOk && !m_unpackStartError && !m_unpackSpaceError &&
			(m_unpackDecryptError || m_unpackPasswordError) &&
			infile.ReadLine(password, sizeof(password) - 1))
#endif
		{
			debug("Password line: %s", password);
			// trim trailing <CR> and <LF>
			char* end = password + strlen(password) - 1;
			while (end >= password && (*end == '\n' || *end == '\r')) *end-- = '\0';

			if (!Util::EmptyStr(password))
			{
#ifdef NZBGET_USE_RUST
				if (nzbget_rs_ucf_unpack_ctrl_should_resume_before_password_try(
					IsStopped() ? 1 : 0, m_autoTerminated ? 1 : 0) != 0)
#else
				if (IsStopped() && m_autoTerminated)
#endif
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
#ifdef NZBGET_USE_RUST
		m_passListTried = nzbget_rs_ucf_unpack_ctrl_eval_pass_list_tried(
			IsStopped() ? 1 : 0, m_autoTerminated ? 1 : 0) != 0;
#else
		m_passListTried = !IsStopped() || m_autoTerminated;
#endif
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

#ifdef NZBGET_USE_RUST
	if (nzbget_rs_pur_unpack_ctrl_should_add_extract_cmd_flag(
		params.Exists("x") ? 1 : 0,
		params.Exists("e") ? 1 : 0) != 0)
#else
	if (!params.Exists("x") && !params.Exists("e"))
#endif
	{
		params.emplace_back("x");
	}

	params.emplace_back("-y");

	if (!Util::EmptyStr(password))
	{
		params.push_back(CString::FormatStr("-p%s", password));
	}
	else
	{
		params.emplace_back("-p-");
	}

#ifdef NZBGET_USE_RUST
	if (nzbget_rs_pur_unpack_ctrl_should_add_default_overwrite_flag(
		params.Exists("-o+") ? 1 : 0,
		params.Exists("-o-") ? 1 : 0,
		params.Exists("-or") ? 1 : 0) != 0)
#else
	if (!params.Exists("-o+") && !params.Exists("-o-") && !params.Exists("-or"))
#endif
	{
		params.emplace_back("-o+");
	}

	params.emplace_back("*.rar");
	m_unpackExtendedDir = FileSystem::MakeExtendedPath(m_unpackDir, true);
	params.push_back(*BString<1024>("%s%c", *m_unpackExtendedDir, PATH_SEPARATOR));
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

#ifdef NZBGET_USE_RUST
	int okFlag = 0, startErr = 0, spaceErr = 0, passErr = 0;
	nzbget_rs_dur_unpack_ctrl_eval_unrar_exit(
		exitCode, m_allOkMessageReceived ? 1 : 0, GetTerminated() ? 1 : 0,
		m_autoTerminated ? 1 : 0, m_unpackPasswordError ? 1 : 0,
		&okFlag, &startErr, &spaceErr, &passErr);
	m_unpackOk = okFlag != 0;
	m_unpackStartError = startErr != 0;
	m_unpackSpaceError = spaceErr != 0;
	m_unpackPasswordError = passErr != 0;
#else
	m_unpackOk = exitCode == 0 && m_allOkMessageReceived && !GetTerminated();
	m_unpackStartError = exitCode == -1 && !m_autoTerminated;
	m_unpackSpaceError = exitCode == 5;
	m_unpackPasswordError |= exitCode == 11; // only for rar5-archives
#endif

#ifdef NZBGET_USE_RUST
	if (nzbget_rs_pnd_unpack_ctrl_should_log_exit_error(
		m_unpackOk ? 1 : 0, exitCode, WillRetryWithPass() ? 1 : 0,
		IsStopped() ? 1 : 0, GetTerminated() ? 1 : 0) != 0)
#else
	if (!m_unpackOk && exitCode > 0 && !WillRetryWithPass() && !IsStopped() && !GetTerminated())
#endif
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

#ifdef NZBGET_USE_RUST
	if (nzbget_rs_pur_unpack_ctrl_should_add_extract_cmd_flag(
		params.Exists("x") ? 1 : 0,
		params.Exists("e") ? 1 : 0) != 0)
#else
	if (!params.Exists("x") && !params.Exists("e"))
#endif
	{
		params.emplace_back("x");
	}

	params.emplace_back("-y");

	if (!Util::EmptyStr(password))
	{
		params.push_back(CString::FormatStr("-p%s", password));
	}
	else
	{
		params.emplace_back("-p-");
	}

	params.push_back(CString::FormatStr("-o%s", *m_unpackDir));
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

#ifdef NZBGET_USE_RUST
	int okFlag = 0, startErr = 0;
	nzbget_rs_pup_unpack_ctrl_eval_sevenzip_exit(
		exitCode, m_allOkMessageReceived ? 1 : 0, GetTerminated() ? 1 : 0,
		m_autoTerminated ? 1 : 0, &okFlag, &startErr);
	m_unpackOk = okFlag != 0;
	m_unpackStartError = startErr != 0;
#else
	m_unpackOk = exitCode == 0 && m_allOkMessageReceived && !GetTerminated();
	m_unpackStartError = exitCode == -1 && !m_autoTerminated;
#endif

#ifdef NZBGET_USE_RUST
	if (nzbget_rs_pnd_unpack_ctrl_should_log_exit_error(
		m_unpackOk ? 1 : 0, exitCode, WillRetryWithPass() ? 1 : 0,
		IsStopped() ? 1 : 0, GetTerminated() ? 1 : 0) != 0)
#else
	if (!m_unpackOk && exitCode > 0 && !WillRetryWithPass() && !IsStopped() && !GetTerminated())
#endif
	{
		PrintMessage(Message::mkError, "7-Zip error code: %i", exitCode);
	}
}

bool UnpackController::WillRetryWithPass()
{
#ifdef NZBGET_USE_RUST
	return nzbget_rs_pup_unpack_ctrl_will_retry_with_pass(
		m_unpackDecryptError ? 1 : 0, m_unpackPasswordError ? 1 : 0,
		m_password.Empty() ? 1 : 0, !g_Options->GetUnpackPassFilePath().empty() ? 1 : 0) != 0;
#else
	return (m_unpackDecryptError || m_unpackPasswordError) &&
		m_password.Empty() && !g_Options->GetUnpackPassFilePath().empty();
#endif
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

	FileList groups;
#ifndef NZBGET_USE_RUST
	RegEx regExSplitExt(".*\\.[a-z,0-9]{3}\\.001$");
#endif

	DirBrowser dir(m_destDir);
	while (const char* filename = dir.Next())
	{
		BString<1024> fullFilename("%s%c%s", *m_destDir, PATH_SEPARATOR, filename);

#ifdef NZBGET_USE_RUST
		if (!FileSystem::DirectoryExists(fullFilename) &&
			nzbget_rs_ucf_unpack_ctrl_is_split_first_piece(filename) != 0 &&
			!FileHasRarSignature(fullFilename))
#else
		if (!FileSystem::DirectoryExists(fullFilename) &&
			regExSplitExt.Match(filename) && !FileHasRarSignature(fullFilename))
#endif
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

	// trim extension
	char* extension = strrchr(destBaseName, '.');
	*extension = '\0';

	BString<1024> fullFilename("%s%c%s", *m_destDir, PATH_SEPARATOR, fragBaseName);
	int64 firstSegmentSize = FileSystem::FileSize(fullFilename);

	// Validate joinable file:
	//  - fragments have continuous numbers (no holes);
	//  - fragments have the same size (except of the last fragment);
	//  - the last fragment must be smaller than other fragments,
	//  if it has the same size it is probably not the last and there are missing fragments.

#ifndef NZBGET_USE_RUST
	RegEx regExSplitExt(".*\\.[a-z,0-9]{3}\\.[0-9]{3}$");
#endif
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
#ifdef NZBGET_USE_RUST
		size_t baseLen = strlen(destBaseName);
		if (!FileSystem::DirectoryExists(fullFilename) &&
			strlen(filename) == baseLen + 4 && !strncasecmp(filename, destBaseName, baseLen))
		{
			int64 segmentSize = FileSystem::FileSize(fullFilename);
			nzbget_rs_pnd_unpack_ctrl_accumulate_split_piece(
				filename, destBaseName, segmentSize, firstSegmentSize,
				&count, &min, &max, &difSizeMin);
		}
#else
		size_t baseLen = strlen(destBaseName);
		if (!FileSystem::DirectoryExists(fullFilename) && regExSplitExt.Match(filename) &&
			strlen(filename) == baseLen + 4 && !strncasecmp(filename, destBaseName, baseLen))
		{
			const char* segExt = strrchr(filename, '.');
			int segNum = atoi(segExt + 1);
			count++;
			min = segNum < min || min == -1 ? segNum : min;
			max = segNum > max ? segNum : max;

			int64 segmentSize = FileSystem::FileSize(fullFilename);
			if (segmentSize != firstSegmentSize)
			{
				difSizeMin = segNum < difSizeMin ? segNum : difSizeMin;
			}
		}
#endif
	}

#ifdef NZBGET_USE_RUST
	if (nzbget_rs_unpack_check_joinable_fragments(
		count, min, max, difSizeMin,
		m_postInfo->GetNzbInfo()->GetParStatus() == NzbInfo::psSuccess ? 1 : 0) == 0)
#else
	int correctedCount = count - (min == 0 ? 1 : 0);
	if ((min > 1) || correctedCount != max ||
		((difSizeMin != correctedCount || difSizeMin > max) &&
		 m_postInfo->GetNzbInfo()->GetParStatus() != NzbInfo::psSuccess))
#endif
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
		PrintMessage(Message::mkInfo, "Joining from %s.%.3i", *destBaseName, i);
		SetProgressLabel(BString<1024>("Joining from %s.%.3i", *destBaseName, i));

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
#ifdef NZBGET_USE_RUST
				int progress = nzbget_rs_ucf_unpack_ctrl_calc_join_progress(
					written, cnt, totalSize, &written);
				m_postInfo->SetStageProgress(progress);
#else
				written += std::max(cnt, 0);
				m_postInfo->SetStageProgress(int(written * 1000 / totalSize));
#endif
			}
			inFile.Close();
			if (!ok)
			{
				break;
			}

			CString fragFilename;
			fragFilename.Format("%s.%.3i", *destBaseName, i);
			m_joinedFiles.push_back(std::move(fragFilename));
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
#ifdef NZBGET_USE_RUST
	if (nzbget_rs_ucf_unpack_ctrl_is_join_size_mismatch(ok ? 1 : 0, written, totalSize) != 0)
#else
	if (ok && written != totalSize)
#endif
	{
		PrintMessage(Message::mkError, "Could not join splitted file %s: joined %lli of %lli bytes",
			*destBaseName, (long long)written, (long long)totalSize);
		ok = false;
	}

	return ok;
}

void UnpackController::Completed()
{
#ifdef NZBGET_USE_RUST
	bool stopped = nzbget_rs_pup_unpack_ctrl_is_stopped_for_reload(
		m_autoTerminated ? 1 : 0, (m_postInfo && m_postInfo->GetDeleted()) ? 1 : 0,
		IsStopped() ? 1 : 0, GetTerminated() ? 1 : 0) != 0;
#else
	bool stopped = !m_autoTerminated && (!m_postInfo || !m_postInfo->GetDeleted()) &&
		(IsStopped() || GetTerminated());
#endif
	if (stopped)
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
#ifdef NZBGET_USE_RUST
		if (nzbget_rs_pup_unpack_ctrl_should_request_par_after_fail(
			m_unpackOk ? 1 : 0, (int)m_postInfo->GetNzbInfo()->GetParStatus(),
			(int)NzbInfo::psSkipped, m_postInfo->GetNzbInfo()->GetParFull() ? 1 : 0,
			m_unpackStartError ? 1 : 0, m_unpackSpaceError ? 1 : 0, m_unpackPasswordError ? 1 : 0,
			GetTerminated() ? 1 : 0, IsStopped() ? 1 : 0, m_autoTerminated ? 1 : 0,
			m_hasParFiles ? 1 : 0) != 0)
		{
			RequestParCheck(nzbget_rs_pup_unpack_ctrl_calc_force_repair_on_request(
				m_password.Empty() ? 1 : 0,
				Util::EmptyStr(g_Options->GetUnpackPassFile()) ? 1 : 0,
				m_passListTried ? 1 : 0,
				m_unpackDecryptError ? 1 : 0,
				m_unpackPasswordError ? 1 : 0,
				(int)m_postInfo->GetNzbInfo()->GetParStatus(),
				(int)NzbInfo::psSkipped) != 0);
		}
#else
		if (!m_unpackOk &&
			(m_postInfo->GetNzbInfo()->GetParStatus() <= NzbInfo::psSkipped ||
			 !m_postInfo->GetNzbInfo()->GetParFull()) &&
			!m_unpackStartError && !m_unpackSpaceError && !m_unpackPasswordError &&
			(!GetTerminated() || m_autoTerminated) &&
			(!IsStopped() || m_autoTerminated) && m_hasParFiles)
		{
			RequestParCheck(!m_password.Empty() ||
				Util::EmptyStr(g_Options->GetUnpackPassFile()) || m_passListTried ||
				!(m_unpackDecryptError || m_unpackPasswordError) ||
				m_postInfo->GetNzbInfo()->GetParStatus() > NzbInfo::psSkipped);
		}
#endif
		else
#endif
		{
			PrintMessage(Message::mkError, "%s failed", *m_infoNameUp);
#ifdef NZBGET_USE_RUST
			m_postInfo->GetNzbInfo()->SetUnpackStatus(
				(NzbInfo::EPostUnpackStatus)nzbget_rs_dur_unpack_ctrl_resolve_fail_status(
					m_unpackSpaceError ? 1 : 0, m_unpackPasswordError ? 1 : 0, m_unpackDecryptError ? 1 : 0,
					(int)NzbInfo::usSpace, (int)NzbInfo::usPassword, (int)NzbInfo::usFailure));
#else
			m_postInfo->GetNzbInfo()->SetUnpackStatus(
				m_unpackSpaceError ? NzbInfo::usSpace :
				m_unpackPasswordError || m_unpackDecryptError ? NzbInfo::usPassword :
				NzbInfo::usFailure);
#endif
		}
	}
}

#ifndef DISABLE_PARCHECK
void UnpackController::RequestParCheck(bool forceRepair)
{
	PrintMessage(Message::mkInfo, "%s requested %s", *m_infoNameUp, forceRepair ? "par-check with forced repair" : "par-check/repair");
	m_postInfo->SetRequestParCheck(true);
	m_postInfo->SetForceRepair(forceRepair);
	m_postInfo->SetUnpackTried(true);
	m_postInfo->SetPassListTried(m_passListTried);
#ifdef NZBGET_USE_RUST
	m_postInfo->SetLastUnpackStatus(nzbget_rs_dur_unpack_ctrl_resolve_fail_status(
		m_unpackSpaceError ? 1 : 0, m_unpackPasswordError ? 1 : 0, m_unpackDecryptError ? 1 : 0,
		(int)NzbInfo::usSpace, (int)NzbInfo::usPassword, (int)NzbInfo::usFailure));
#else
	m_postInfo->SetLastUnpackStatus((int)(m_unpackSpaceError ? NzbInfo::usSpace :
		m_unpackPasswordError || m_unpackDecryptError ? NzbInfo::usPassword :
		NzbInfo::usFailure));
#endif
}
#endif

void UnpackController::CreateUnpackDir()
{
#ifdef NZBGET_USE_RUST
	bool useInterDir = nzbget_rs_dur_direct_unpack_use_inter_dir(
		m_postInfo->GetNzbInfo()->GetDestDir(), g_Options->GetInterDir(), PATH_SEPARATOR) != 0;
#else
	bool useInterDir = !Util::EmptyStr(g_Options->GetInterDir()) &&
		!strncmp(m_postInfo->GetNzbInfo()->GetDestDir(), g_Options->GetInterDir(), strlen(g_Options->GetInterDir())) &&
		m_postInfo->GetNzbInfo()->GetDestDir()[strlen(g_Options->GetInterDir())] == PATH_SEPARATOR;
#endif

	if (useInterDir && m_finalDir.Empty())
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
		m_unpackDir.Format("%s", destDir);
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
#ifndef NZBGET_USE_RUST
	RegEx regExRar(".*\\.rar$");
	RegEx regExRarMultiSeq(".*\\.[r-z][0-9][0-9]$");
	RegEx regExSevenZip(".*\\.7z$");
	RegEx regExSevenZipMulti(".*\\.7z\\.[0-9]+$");
	RegEx regExSplitExt(".*\\.[a-z,0-9]{3}\\.[0-9]{3}$");
#endif

	DirBrowser dir(m_destDir);
	while (const char* filename = dir.Next())
	{
		BString<1024> fullFilename("%s%c%s", *m_destDir, PATH_SEPARATOR, filename);

		if (!FileSystem::DirectoryExists(fullFilename) && IsDownloadedFile(filename))
		{
#ifdef NZBGET_USE_RUST
			const int archiveKind = nzbget_rs_dur_unpack_ctrl_classify_archive_file(filename);
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
#else
			const char* ext = strrchr(filename, '.');
			int extNum = ext ? atoi(ext + 1) : -1;

			if (regExRar.Match(filename))
			{
				const auto begin = m_postInfo->GetExtractedArchives()->cbegin();
				const auto end = m_postInfo->GetExtractedArchives()->cend();
				m_hasRarFiles = true;
				m_hasNotUnpackedRarFiles |= std::find(begin, end, filename) == end;
			}
			else if (regExSevenZip.Match(filename))
			{
				m_hasSevenZipFiles = true;
			}
			else if (regExSevenZipMulti.Match(filename))
			{
				m_hasSevenZipMultiFiles = true;
			}
			else if (regExSplitExt.Match(filename) && (extNum == 0 || extNum == 1))
			{
				m_hasSplittedFiles = true;
			}
			else if (!m_hasRenamedArchiveFiles && !regExRarMultiSeq.Match(filename) &&
				!Util::MatchFileExt(filename, g_Options->GetUnpackIgnoreExt(), ",;") &&
				FileHasRarSignature(fullFilename))
			{
				m_hasRenamedArchiveFiles = true;
			}
#endif
		}
	}
}

// Without a temporary unpack folder, extracted files land next to the downloaded
// ones (and an archive may hold archives): only the download's own files count as
// its archives. Without a record of them, every file does.
bool UnpackController::IsDownloadedFile(const char* filename)
{
#ifdef NZBGET_USE_RUST
	return nzbget_rs_ucf_unpack_ctrl_is_downloaded_file_allowed(
		g_Options->GetUseTempUnpackDir() ? 1 : 0,
		m_downloadedFiles.empty() ? 1 : 0) != 0 ||
		m_downloadedFiles.Exists(filename);
#else
	return g_Options->GetUseTempUnpackDir() || m_downloadedFiles.empty() ||
		m_downloadedFiles.Exists(filename);
#endif
}

bool UnpackController::FileHasRarSignature(const char* filename)
{
#ifdef NZBGET_USE_RUST
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
#else
	char rar4Signature[] = { 0x52, 0x61, 0x72, 0x21, 0x1A, 0x07, 0x00 };
	char rar5Signature[] = { 0x52, 0x61, 0x72, 0x21, 0x1A, 0x07, 0x01, 0x00 };

	char fileSignature[8]{};

	int cnt = 0;
	DiskFile infile;
	if (infile.Open(filename, DiskFile::omRead))
	{
		cnt = (int)infile.Read(fileSignature, sizeof(fileSignature));
		infile.Close();
	}

	bool rar = cnt == sizeof(fileSignature) &&
		(!strcmp(rar4Signature, fileSignature) || !strcmp(rar5Signature, fileSignature));
	return rar;
#endif
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

#ifdef NZBGET_USE_RUST
	if (nzbget_rs_ucf_unpack_ctrl_should_move_from_temp_unpack(
		m_unpackOk ? 1 : 0, g_Options->GetUseTempUnpackDir() ? 1 : 0) != 0)
#else
	if (m_unpackOk && g_Options->GetUseTempUnpackDir())
#endif
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

#ifdef NZBGET_USE_RUST
			bool hiddenFile = nzbget_rs_ucf_move_ctrl_is_dot_file(filename) != 0;
#else
			bool hiddenFile = filename[0] == '.';
#endif

			if (!FileSystem::MoveFile(srcFile, dstFile) && !hiddenFile)
			{
				PrintMessage(Message::mkError, "Could not move file %s to %s: %s", *srcFile, *dstFile,
					*FileSystem::GetLastErrorMessage());
				ok = false;
			}

#ifndef WIN32
			// Fixing file or directory permissions overridden by the unpacker
			FileSystem::RestoreFileOrDirPermissions(dstFile.Str());
#endif

			extractedFiles.push_back(filename);
		}

#ifndef WIN32
		// Fixing directory permissions overridden by the unpacker
		FileSystem::RestoreDirPermissions(destDir);
#endif

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

#ifdef NZBGET_USE_RUST
	if (nzbget_rs_ucf_unpack_ctrl_should_cleanup_archive_disks(
		m_unpackOk ? 1 : 0, ok ? 1 : 0, g_Options->GetUnpackCleanupDisk() ? 1 : 0) != 0)
#else
	if (m_unpackOk && ok && g_Options->GetUnpackCleanupDisk())
#endif
	{
		PrintMessage(Message::mkInfo, "Deleting archive files");

#ifndef NZBGET_USE_RUST
		RegEx regExRar(".*\\.rar$");
		RegEx regExRarMultiSeq(".*\\.[r-z][0-9][0-9]$");
		RegEx regExSevenZip(".*\\.7z$|.*\\.7z\\.[0-9]+$");
		RegEx regExSplitExt(".*\\.[a-z,0-9]{3}\\.[0-9]{3}$");
#endif

		DirBrowser dir(m_destDir);
		while (const char* filename = dir.Next())
		{
			BString<1024> fullFilename("%s%c%s", *m_destDir, PATH_SEPARATOR, filename);

#ifdef NZBGET_USE_RUST
			int cleanupKind = nzbget_rs_pnd_unpack_ctrl_classify_cleanup_archive_file(
				FileSystem::DirectoryExists(fullFilename) ? 1 : 0,
				m_interDir ? 1 : 0,
				extractedFiles.Exists(filename) ? 1 : 0,
				IsDownloadedFile(filename) ? 1 : 0,
				m_hasSplittedFiles ? 1 : 0,
				m_joinedFiles.Exists(filename) ? 1 : 0,
				filename);
			if (cleanupKind == 1 || (cleanupKind == 2 && FileHasRarSignature(fullFilename)))
#else
			if (!FileSystem::DirectoryExists(fullFilename) &&
				(m_interDir || !extractedFiles.Exists(filename)) && IsDownloadedFile(filename) &&
				(regExRar.Match(filename) || regExSevenZip.Match(filename) ||
				 (regExRarMultiSeq.Match(filename) && FileHasRarSignature(fullFilename)) ||
				 (m_hasSplittedFiles && regExSplitExt.Match(filename) && m_joinedFiles.Exists(filename))))
#endif
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
	bool printed = false;

	int i = 0;

	for (; i < bufSize - 1; i++)
	{
		int ch = fgetc(stream);
		buf[i] = ch;
		buf[i+1] = '\0';
		if (ch == EOF)
		{
			break;
		}
		if (ch == '\n')
		{
			i++;
			break;
		}

		char* backspace = strrchr(buf, '\b');
		if (backspace)
		{
			if (!printed)
			{
				BString<1024> tmp = buf;
				char* tmpPercent = strrchr(tmp, '\b');
				if (tmpPercent)
				{
					*tmpPercent = '\0';
				}
#ifdef NZBGET_USE_RUST
				if (nzbget_rs_ucf_direct_unpack_is_progress_dots_line(buf) == 0)
#else
				if (strncmp(buf, "...", 3))
#endif
				{
					ProcessOutput(tmp);
				}
				printed = true;
			}
#ifdef NZBGET_USE_RUST
			int progress = 0;
			if (nzbget_rs_ufs_unpack_ctrl_parse_backspace_progress(backspace, &progress) != 0)
			{
				m_postInfo->SetStageProgress(progress);
			}
#else
			if (strchr(backspace, '%'))
			{
				int percent = atoi(backspace + 1);
				m_postInfo->SetStageProgress(percent * 10);
			}
#endif
		}
	}

	buf[i] = '\0';

	if (printed)
	{
		buf[0] = '\0';
	}

	return i > 0;
}

void UnpackController::AddMessage(Message::EKind kind, const char* text)
{
	BString<1024> msgText = text;
#ifdef NZBGET_USE_RUST
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

	if (!IsStopped() && (m_unpackDecryptError || m_unpackPasswordError || info.crc_or_volume_error != 0))
	{
		if (m_postInfo && !WillRetryWithPass())
		{
			m_postInfo->GetNzbInfo()->AddMessage(
				Message::mkWarning,
				BString<1024>("Cancelling %s due to errors", *m_infoName)
			);
		}

		m_autoTerminated = true;
		Stop();
	}

	if (info.all_ok != 0)
	{
		m_allOkMessageReceived = true;
	}
#else
	int len = strlen(text);

	// Modify unrar messages for better readability:
	// remove the destination path part from message "Extracting file.xxx"
	if (m_unpacker == upUnrar && !strncmp(text, "Unrar: Extracting  ", 19) &&
		!strncmp(text + 19, m_unpackExtendedDir, strlen(m_unpackExtendedDir)))
	{
		msgText.Format("Unrar: Extracting %s", text + 19 + strlen(m_unpackExtendedDir) + 1);
	}

	if (m_postInfo)
	{
		m_postInfo->GetNzbInfo()->AddMessage(kind, msgText);
	}

	if (m_postInfo && m_unpacker == upUnrar && !strncmp(msgText, "Unrar: UNRAR ", 13) &&
		strstr(msgText, " Copyright ") && strstr(msgText, " Alexander Roshal"))
	{
		// reset start time for a case if user uses unpack-script to do some things
		// (like sending Wake-On-Lan message) before executing unrar
		m_postInfo->SetStageTime(Util::CurrentTime());
	}

	if (m_postInfo && m_unpacker == upUnrar && !strncmp(msgText, "Unrar: Extracting ", 18))
	{
		SetProgressLabel(msgText + 7);
	}

	if (m_postInfo && m_unpacker == upUnrar && !strncmp(text, "Unrar: Extracting from ", 23))
	{
#ifdef DEBUG
		const char *filename = text + 23;
		debug("Filename: %s", filename);
#endif
		SetProgressLabel(text + 7);
	}

	if (m_unpacker == upUnrar &&
		(!strncmp(text, "Unrar: Checksum error in the encrypted file", 42) ||
		 !strncmp(text, "Unrar: CRC failed in the encrypted file", 39)))
	{
		m_unpackDecryptError = true;
	}

	if (m_unpacker == upUnrar && (!strncmp(text, "Unrar: The specified password is incorrect.", 43) ||
		!strncmp(text, "Unrar: Incorrect password for", 29)))
	{
		m_unpackPasswordError = true;
	}

	if (m_unpacker == upSevenZip &&
		(strstr(text, "Cannot open encrypted archive. Wrong password?") ||
		 strstr(text, "Data Error in encrypted file. Wrong password?") ||
		 strstr(text, "CRC Failed in encrypted file. Wrong password?")))
	{
		m_unpackDecryptError = true;
	}

	if (!IsStopped() && (m_unpackDecryptError || m_unpackPasswordError ||
		strstr(text, " : packed data CRC failed in volume") ||
		strstr(text, " : packed data checksum error in volume") ||
		(len > 13 && !strncmp(text + len - 13, " - CRC failed", 13)) ||
		(len > 18 && !strncmp(text + len - 18, " - checksum failed", 18)) ||
		!strncmp(text, "Unrar: WARNING: You need to start extraction from a previous volume", 67)))
	{
		if (m_postInfo && !WillRetryWithPass())
		{
			m_postInfo->GetNzbInfo()->AddMessage(
				Message::mkWarning,
				BString<1024>("Cancelling %s due to errors", *m_infoName)
			);
		}

		m_autoTerminated = true;
		Stop();
	}

	if ((m_unpacker == upUnrar && !strncmp(text, "Unrar: All OK", 13)) ||
		(m_unpacker == upSevenZip && !strncmp(text, "7-Zip: Everything is Ok", 23)))
	{
		m_allOkMessageReceived = true;
	}
#endif
}

void UnpackController::Stop()
{
	debug("Stopping unpack");
	Thread::Stop();
	bool isExecThread = std::this_thread::get_id() == m_execThreadId.load();
#ifdef NZBGET_USE_RUST
	if (m_unpacking && nzbget_rs_ucf_direct_unpack_should_try_break(
		isExecThread ? 1 : 0, m_autoTerminated ? 1 : 0, HasActiveProcess() ? 1 : 0) != 0)
#else
	if (!isExecThread && !m_autoTerminated && m_unpacking && HasActiveProcess())
#endif
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
#ifdef NZBGET_USE_RUST
	for (CompletedFile& completedFile: nzbInfo->GetCompletedFiles())
	{
		if (nzbget_rs_dur_unpack_ctrl_has_completed_archive_ext(completedFile.GetFilename()) != 0)
		{
			return true;
		}
	}
	return false;
#else
	RegEx regExRar(".*\\.rar$");
	RegEx regExSevenZip(".*\\.7z$");
	RegEx regExSevenZipMulti(".*\\.7z\\.[0-9]+$");

	for (CompletedFile& completedFile: nzbInfo->GetCompletedFiles())
	{
		const char* filename = completedFile.GetFilename();
		if (regExRar.Match(filename) ||
			regExSevenZip.Match(filename) ||
			regExSevenZipMulti.Match(filename))
		{
			return true;
		}
	}

	return false;
#endif
}
