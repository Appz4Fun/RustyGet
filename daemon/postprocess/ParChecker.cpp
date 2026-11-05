/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2007-2019 Andrey Prygunkov <hugbug@users.sourceforge.net>
 *  Copyright (C) 2024-2026 Denis <denis@nzbget.com>
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

#include <vector>
#include <string>
#include <algorithm>

#include <par2/libpar2.h>
#include <par2/commandline.h>

#include "ParChecker.h"
#include "ParParser.h"
#include "Log.h"
#include "Options.h"
#include "Util.h"
#include "FileSystem.h"
#include "nzbget_rs.h"

class Repairer final : public Par2::Par2Repairer, public ParChecker::AbstractRepairer
{
public:
	Repairer(ParChecker* owner)
		: Par2::Par2Repairer(owner->m_parCout, owner->m_parCerr, Par2::nlNormal)
		, m_owner(owner)
		, m_commandLine() 
	{
		int configuredParBuffer = g_Options->GetParBuffer();
		int effectiveMem = g_MemoryMonitor
			? g_MemoryMonitor->GetEffectiveParBufferMB(configuredParBuffer)
			: (configuredParBuffer > 0 ? configuredParBuffer : 16);
		nzbget_rs_dur_parchecker_calc_threads_and_mem(
			g_Options->GetParThreads(), Util::NumberOfCpuCores(), effectiveMem,
			&m_threadsToUse, &m_memToUse);
	}
	Par2::Result PreProcess(const std::string& parFilename);
	Par2::Result Process(bool dorepair);
	Repairer* GetRepairer() override { return this; }

protected:
	void SigFilename(std::string filename) override { m_owner->SignalFilename(std::move(filename)); }
	void SigProgress(int progress) override { m_owner->SignalProgress(progress); }
	void SigDone(std::string filename, int available, int total) override { m_owner->SignalDone(std::move(filename), available, total); }

	bool ScanDataFile(
		Par2::DiskFile *diskfile,
		std::string basepath,
		const bool renameonly,
		Par2::Par2RepairerSourceFile* &sourcefile,
		Par2::MatchType &matchtype, 
		Par2::MD5Hash &hashfull, 
		Par2::MD5Hash &hash16k, 
		Par2::u32 &count) override;

private:
	typedef std::vector<Thread*> Threads;

	ParChecker* m_owner;
	Par2::CommandLine m_commandLine;
	int m_threadsToUse;
	int m_memToUse;

	void BeginRepair() override;

	friend class ParChecker;
};

class RepairCreatorPacket : public Par2::CreatorPacket
{
	friend class ParChecker;
};

Par2::Result Repairer::PreProcess(const std::string& parFilename)
{
	NzbgetRsBuf memBuf{}, threadsBuf{}, fileThreadsBuf{};
	nzbget_rs_dqh_parchecker_format_cli_flags(
		m_threadsToUse, m_memToUse, &memBuf, &threadsBuf, &fileThreadsBuf);
	std::string memParam(memBuf.data ? memBuf.data : "", memBuf.len);
	std::string threadsParam(threadsBuf.data ? threadsBuf.data : "", threadsBuf.len);
	std::string fileThreadsParam(fileThreadsBuf.data ? fileThreadsBuf.data : "", fileThreadsBuf.len);
	nzbget_rs_free(memBuf);
	nzbget_rs_free(threadsBuf);
	nzbget_rs_free(fileThreadsBuf);

	if (g_Options->GetParScan() == Options::psFull)
	{
		// the path is the text, not a format (a "%" in a name)
		NzbgetRsBuf wildcardBuf = nzbget_rs_pup_parchecker_build_wildcard_param(parFilename.c_str());
		BString<1024> wildcardParam;
		if (wildcardBuf.data)
		{
			wildcardParam.Set(wildcardBuf.data, static_cast<int>(wildcardBuf.len));
			nzbget_rs_free(wildcardBuf);
		}

		const char* argv[] = { "par2", "r", "-v", memParam.c_str(), threadsParam.c_str(), fileThreadsParam.c_str(), parFilename.c_str(), wildcardParam };
		if (!m_commandLine.Parse(8, const_cast<char**>(argv)))
		{
			return Par2::eInvalidCommandLineArguments;
		}
	}
	else
	{
		const char* argv[] = { "par2", "r", "-v", memParam.c_str(), threadsParam.c_str(), fileThreadsParam.c_str(), parFilename.c_str() };
		if (!m_commandLine.Parse(7, const_cast<char**>(argv)))
		{
			return Par2::eInvalidCommandLineArguments;
		}
	}

	return Par2Repairer::PreProcess(m_commandLine);
}

Par2::Result Repairer::Process(bool dorepair)
{
	Par2::Result res = Par2Repairer::Process(
		m_commandLine.GetMemoryLimit(),
		m_commandLine.GetBasePath(),
		m_commandLine.GetNumThreads(),
		m_commandLine.GetFileThreads(),
		m_commandLine.GetParFilename(),
		m_commandLine.GetExtraFiles(),
		dorepair,
		m_commandLine.GetPurgeFiles(),
		false,
		m_commandLine.GetSkipData(),
		m_commandLine.GetSkipLeaway());
	debug("ParChecker: Process-result=%i", res);
	return res;
}


bool Repairer::ScanDataFile(
	Par2::DiskFile *diskfile,
	std::string basepath,
	const bool renameonly,
	Par2::Par2RepairerSourceFile* &sourcefile,
	Par2::MatchType &matchtype, 
	Par2::MD5Hash &hashfull, 
	Par2::MD5Hash &hash16k, 
	Par2::u32 &count)
{
	if (nzbget_rs_srp_parchecker_can_quick_scan_data_file(
		m_owner->GetParQuick() ? 1 : 0,
		sourcefile != nullptr ? 1 : 0,
		diskfile != nullptr ? 1 : 0))
	{
		std::string name;
		Par2::DiskFile::SplitFilename(diskfile->FileName(), basepath, name);

		SigFilename(name);

		if (nzbget_rs_dps_parchecker_should_quick_verify(
			(int)m_owner->GetStage(), (int)ParChecker::ptVerifyingRepaired, m_owner->GetParFull() ? 1 : 0) != 0)
		{
			int availableBlocks = static_cast<int>(sourcefile->BlockCount());
			ParChecker::EFileStatus fileStatus = m_owner->VerifyDataFile(*diskfile, *sourcefile, availableBlocks);
			if (fileStatus != ParChecker::fsUnknown)
			{
				SigDone(name, availableBlocks, static_cast<int>(sourcefile->BlockCount()));
				SigProgress(1000);
				matchtype = (Par2::MatchType)nzbget_rs_pup_parchecker_select_match_type(
					(int)fileStatus, (int)ParChecker::fsSuccess, (int)ParChecker::fsPartial,
					(int)Par2::eFullMatch, (int)Par2::ePartialMatch, (int)Par2::eNoMatch);
				m_owner->SetParFull(false);
				return true;
			}
		}
	}

	return Par2Repairer::ScanDataFile(
		diskfile, 
		basepath,
		renameonly,
		sourcefile, 
		matchtype,
		hashfull, 
		hash16k, 
		count);
}

void Repairer::BeginRepair()
{
	m_owner->PrintMessage(Message::mkInfo, "Using %i thread(s) and %i MB to repair %u block(s) for %s",
		m_threadsToUse, m_memToUse, missingblockcount, m_owner->m_nzbName.c_str());
}

int ParChecker::StreamBuf::overflow(int ch)
{
	if (ch == '\n' || ch == '\r')
	{
		if (m_buffer.empty())
		{
			return ch;
		}

		char* msg = m_buffer.data();

		if (msg)
		{
			size_t offset = 0;
			bool extraDebug = nzbget_rs_parchecker_classify_streambuf_line(msg, &offset) != 0;
			msg += offset;
			if (extraDebug)
			{
				debug("Par: %s", msg);
			}
			else
			{
				m_owner->PrintMessage(m_kind, "Par: %s", msg);
			}
		}

		m_buffer.clear();
	}
	else
	{
		char bf[2];
		bf[0] = (char)ch;
		bf[1] = '\0';
		m_buffer.append(std::string(bf));
	}
	return ch;
}


void ParChecker::Cleanup()
{
	std::lock_guard guard{ m_repairerMtx };

	m_repairer.reset();
	m_queuedParFiles.clear();
	m_processedFiles.clear();
	m_sourceFiles.clear();
	m_dupeSources.clear();
	m_errMsg = "";

	MemoryMonitor::TrimHeap();
}

void ParChecker::Execute()
{
	m_verifiedFiles.clear();
	m_status = RunParCheckAll();

	if (nzbget_rs_pup_parchecker_should_run_forced_full_check(
		(int)m_status, (int)psRepairNotNeeded, m_parQuick ? 1 : 0,
		m_forceRepair ? 1 : 0, IsStopped() ? 1 : 0) != 0)
	{
		PrintMessage(Message::mkInfo, "Performing full par-check for %s", m_nzbName.c_str());
		m_parQuick = false;
		m_verifiedFiles.clear();
		m_status = RunParCheckAll();
	}

	// Publish only the final pass: a forced full check can overturn a quick
	// result, and its pending duplicate jobs must survive until then.
	for (const std::string& filename : m_verifiedFiles)
	{
		RegisterVerifiedFile(filename.c_str());
	}
	Completed();
}

ParChecker::EStatus ParChecker::RunParCheckAll()
{
	ParParser::ParFileList fileList;
	if (!ParParser::FindMainPars(m_destDir.c_str(), &fileList))
	{
		PrintMessage(Message::mkError, "Could not start par-check for %s. Could not find any par-files", m_nzbName.c_str());
		return psFailed;
	}

	EStatus allStatus = psRepairNotNeeded;
	m_parFull = true;

	for (std::string& parFilename : fileList)
	{
		debug("Found par: %s", parFilename.c_str());

		if (!IsStopped())
		{
			NzbgetRsBuf pathBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
				m_destDir.c_str(), PATH_SEPARATOR, parFilename.c_str());
			std::string path(pathBuf.data ? pathBuf.data : "", pathBuf.len);
			nzbget_rs_free(pathBuf);
			std::string fullParFilename = FileSystem::GetRealPath(path)
				.value_or(std::move(path));

			int baseLen = 0;
			ParParser::ParseParFilename(parFilename.c_str(), true, &baseLen, nullptr);
			BString<1024> infoName;
			infoName.Set(parFilename.c_str(), baseLen);

			NzbgetRsBuf parInfoBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
				m_nzbName.c_str(), PATH_SEPARATOR, *infoName);
			BString<1024> parInfoName;
			parInfoName.Set(parInfoBuf.data ? parInfoBuf.data : "", (int)parInfoBuf.len);
			nzbget_rs_free(parInfoBuf);
			SetInfoName(parInfoName);

			EStatus status = RunParCheck(std::move(fullParFilename));

			// accumulate total status, the worst status has priority
			allStatus = (EStatus)nzbget_rs_pnd_parchecker_accumulate_worst_status((int)allStatus, (int)status);
		}
	}

	return allStatus;
}

ParChecker::EStatus ParChecker::RunParCheck(std::string parFilename)
{
	Cleanup();
	m_parFilename = std::move(parFilename);
	m_stage = ptLoadingPars;
	m_processedCount = 0;
	m_extraFiles = 0;
	m_quickFiles = 0;
	m_verifyingExtraFiles = false;
	m_hasDamagedFiles = false;
	EStatus status = psFailed;

	PrintMessage(Message::mkInfo, "Verifying %s", m_infoName.c_str());

	debug("par: %s", m_parFilename.c_str());

	NzbgetRsBuf verifyLabelBuf = nzbget_rs_dqh_parchecker_format_action_label("Verifying", m_infoName.c_str());
	m_progressLabel.assign(verifyLabelBuf.data ? verifyLabelBuf.data : "", verifyLabelBuf.len);
	nzbget_rs_free(verifyLabelBuf);
	m_fileProgress = 0;
	m_stageProgress = 0;
	UpdateProgress();

	Par2::Result res = (Par2::Result)PreProcessPar();
	if (IsStopped() || res != Par2::eSuccess)
	{
		Cleanup();
		return psFailed;
	}

	std::string creator = GetPacketCreator();
	info("Recovery files created by: %s", nzbget_rs_dqh_parchecker_creator_or_unknown(creator.c_str()));

	m_stage = ptVerifyingSources;
	res = GetRepairer()->Process(false);

	if (!m_parQuick)
	{
		CheckEmptyFiles();
	}

	bool addedSplittedFragments = false;
	if (nzbget_rs_pnd_parchecker_should_try_more_recovery(
		m_hasDamagedFiles ? 1 : 0, IsStopped() ? 1 : 0, (int)res,
		(int)Par2::eRepairNotPossible, 0, 0, 0) != 0)
	{
		addedSplittedFragments = AddSplittedFragments();
		if (addedSplittedFragments)
		{
			res = GetRepairer()->Process(false);
		}
	}

	if (nzbget_rs_pup_parchecker_should_add_missing_files(
		m_hasDamagedFiles ? 1 : 0, IsStopped() ? 1 : 0, GetRepairer()->missingfilecount,
		addedSplittedFragments ? 1 : 0, (int)res, (int)Par2::eRepairPossible,
		(int)g_Options->GetParScan(), (int)Options::psExtended, (int)Options::psDupe) != 0)
	{
		if (AddMissingFiles())
		{
			res = GetRepairer()->Process(false);
		}
	}

	if (nzbget_rs_pnd_parchecker_should_try_more_recovery(
		m_hasDamagedFiles ? 1 : 0, IsStopped() ? 1 : 0, (int)res,
		(int)Par2::eRepairNotPossible, 0, 0, 0) != 0)
	{
		res = static_cast<Par2::Result>(ProcessMorePars());
	}

	if (nzbget_rs_pnd_parchecker_should_try_more_recovery(
		m_hasDamagedFiles ? 1 : 0, IsStopped() ? 1 : 0, (int)res,
		(int)Par2::eRepairNotPossible, 1, (int)g_Options->GetParScan(), (int)Options::psDupe) != 0)
	{
		if (AddDupeFiles())
		{
			res = GetRepairer()->Process(false);
			if (nzbget_rs_src_parchecker_should_process_twin_missing_blocks(
				(int)res, (int)Par2::eRepairNotPossible, IsStopped() ? 1 : 0))
			{
				res = static_cast<Par2::Result>(ProcessMorePars());
			}
		}
	}

	// a twin's par2 set (other block size, other parity level, same files) may hold
	// the recovery blocks its own lacks
	if (nzbget_rs_pnd_parchecker_should_try_more_recovery(
		m_hasDamagedFiles ? 1 : 0, IsStopped() ? 1 : 0, (int)res,
		(int)Par2::eRepairNotPossible, 0, 0, 0) != 0)
	{
		res = static_cast<Par2::Result>(ProcessTwinSet());
	}

	if (IsStopped())
	{
		Cleanup();
		return psFailed;
	}

	status = psFailed;

	int postVerifyAction = 0;
	status = (EStatus)nzbget_rs_pnd_parchecker_eval_post_verify_action(
		(int)res, (int)Par2::eSuccess, (int)Par2::eRepairPossible,
		m_hasDamagedFiles ? 1 : 0, g_Options->GetParRepair() ? 1 : 0,
		(int)psFailed, (int)psRepairNotNeeded, (int)psRepairPossible,
		&postVerifyAction);
	if (postVerifyAction == 0)
	{
		PrintMessage(Message::mkInfo, "Repair not needed for %s", m_infoName.c_str());
	}
	else if (postVerifyAction == 1)
	{
		PrintMessage(Message::mkInfo, "Repairing %s", m_infoName.c_str());

		SaveSourceList();
		NzbgetRsBuf repairLabelBuf = nzbget_rs_dqh_parchecker_format_action_label("Repairing", m_infoName.c_str());
		m_progressLabel.assign(repairLabelBuf.data ? repairLabelBuf.data : "", repairLabelBuf.len);
		nzbget_rs_free(repairLabelBuf);
		m_fileProgress = 0;
		m_stageProgress = 0;
		m_processedCount = 0;
		m_stage = ptRepairing;
		m_filesToRepair = nzbget_rs_dps_parchecker_calc_files_to_repair(
			GetRepairer()->damagedfilecount, GetRepairer()->missingfilecount);
		UpdateProgress();

		res = GetRepairer()->Process(true);
		if (res == Par2::eSuccess)
		{
			PrintMessage(Message::mkInfo, "Successfully repaired %s", m_infoName.c_str());
			status = psRepaired;
			StatDupeSources(&m_dupeSources);
			DeleteLeftovers();
		}
		else
		{
			status = psFailed;
		}
	}
	else if (postVerifyAction == 2)
	{
		PrintMessage(Message::mkInfo, "Repair possible for %s", m_infoName.c_str());
	}

	int outcomeKind = 0;
	int useCmdErr = 0;
	status = (EStatus)nzbget_rs_pnd_parchecker_eval_cancel_or_fail_outcome(
		IsStopped() ? 1 : 0, (int)m_stage, (int)ptRepairing, (int)status,
		(int)psFailed, (int)psRepairPossible, m_errMsg.empty() ? 1 : 0, (int)res,
		&outcomeKind, &useCmdErr);
	if (outcomeKind == 1)
	{
		PrintMessage(Message::mkWarning, "Repair cancelled for %s", m_infoName.c_str());
		m_errMsg = "repair cancelled";
	}
	else if (outcomeKind == 2)
	{
		PrintMessage(Message::mkWarning, "Par-check cancelled for %s", m_infoName.c_str());
		m_errMsg = "par-check cancelled";
	}
	else if (outcomeKind == 3)
	{
		if (useCmdErr != 0)
		{
			m_errMsg = nzbget_rs_dqh_parchecker_cmdline_err_str((int)res);
		}
		PrintMessage(Message::mkError, "Repair failed for %s: %s. Recovery files created by: %s",
			m_infoName.c_str(), m_errMsg.c_str(), nzbget_rs_dqh_parchecker_creator_or_unknown(creator.c_str()));
	}

	// A successful set does not cover unrelated files, and ignored damage
	// can also produce psRepairNotNeeded. Keep only complete final targets
	// actually established by libpar2, before its source metadata is freed.
	for (Par2::Par2RepairerSourceFile* sourcefile : GetRepairer()->sourcefiles)
	{
		if (!sourcefile)
		{
			continue;
		}
		// A later set may have attempted to rewrite the same target; its
		// result supersedes any earlier proof, including when repair failed.
		std::string targetName = sourcefile->TargetFileName();
		m_verifiedFiles.erase(std::remove_if(m_verifiedFiles.begin(), m_verifiedFiles.end(),
			[&targetName](const std::string& previous)
			{
				return FileSystem::SameFilename(previous.c_str(), targetName.c_str());
			}), m_verifiedFiles.end());
		if (nzbget_rs_pnd_parchecker_is_verified_complete_target(
			(int)status, (int)psRepaired, (int)psRepairNotNeeded,
			sourcefile->GetCompleteFile() ? 1 : 0,
			sourcefile->GetCompleteFile() == sourcefile->GetTargetFile() ? 1 : 0) != 0)
		{
			m_verifiedFiles.push_back(sourcefile->GetTargetFile()->FileName());
		}
	}

	Cleanup();
	return status;
}

int ParChecker::PreProcessPar()
{
	Par2::Result res = Par2::eRepairFailed;
	while (!IsStopped() && res != Par2::eSuccess)
	{
		Cleanup();

		{
			std::lock_guard guard{ m_repairerMtx };
			m_repairer = std::make_unique<Repairer>(this);
		}

		res = GetRepairer()->PreProcess(m_parFilename);
		debug("ParChecker: PreProcess-result=%i", res);

		if (IsStopped())
		{
			PrintMessage(Message::mkError, "Could not verify %s: stopping", m_infoName.c_str());
			m_errMsg = "par-check was stopped";
			return Par2::eRepairFailed;
		}

		if (res == Par2::eInvalidCommandLineArguments)
		{
			PrintMessage(Message::mkError, "Could not start par-check for %s. Par-file: %s", m_infoName.c_str(), m_parFilename.c_str());
			m_errMsg = "Command line could not be parsed";
			return res;
		}

		if (res != Par2::eSuccess)
		{
			PrintMessage(Message::mkWarning, "Could not verify %s: par2-file could not be processed", m_infoName.c_str());
			PrintMessage(Message::mkInfo, "Requesting more par2-files for %s", m_infoName.c_str());
			bool hasMorePars = LoadMainParBak();
			if (!hasMorePars)
			{
				PrintMessage(Message::mkWarning, "No more par2-files found");
				break;
			}
		}
	}

	if (res != Par2::eSuccess)
	{
		PrintMessage(Message::mkError, "Could not verify %s: par2-file could not be processed", m_infoName.c_str());
		m_errMsg = "par2-file could not be processed";
		return res;
	}

	return res;
}

bool ParChecker::LoadMainParBak()
{
	while (!IsStopped())
	{
		bool hasMorePars = false;
		{
			std::lock_guard guard{ m_queuedParFilesMtx };
			hasMorePars = !m_queuedParFiles.empty();
			m_queuedParFiles.clear();
		}

		if (hasMorePars)
		{
			return true;
		}

		int blockFound = 0;
		bool requested = RequestMorePars(1, &blockFound);
		if (requested)
		{
			m_progressLabel = "Awaiting additional par-files";
			m_fileProgress = 0;
			UpdateProgress();
		}

		{
			std::lock_guard guard{ m_queuedParFilesMtx };
			hasMorePars = !m_queuedParFiles.empty();
			m_queuedParFilesChanged = false;
		}

		int waitAction = nzbget_rs_srp_parchecker_should_wait_for_queued_pars(
			requested ? 1 : 0, hasMorePars ? 1 : 0);
		if (waitAction == 0)
		{
			return false;
		}

		if (waitAction == 1)
		{
			// wait until new files are added by "AddParFile" or a change is signaled by "QueueChanged"
			bool queuedParFilesChanged = false;
			while (!queuedParFilesChanged && !IsStopped())
			{
				{
					std::lock_guard guard{ m_queuedParFilesMtx };
					queuedParFilesChanged = m_queuedParFilesChanged;
				}
				Util::Sleep(100);
			}
		}
	}

	return false;
}

int ParChecker::ProcessMorePars()
{
	Par2::Result res = Par2::eRepairNotPossible;

	bool moreFilesLoaded = true;
	while (!IsStopped() && res == Par2::eRepairNotPossible)
	{
		int missingblockcount = nzbget_rs_pup_parchecker_calc_missing_blocks(
			GetRepairer()->missingblockcount, GetRepairer()->recoverypacketmap.size());
		if (missingblockcount <= 0)
		{
			return Par2::eRepairPossible;
		}

		if (moreFilesLoaded)
		{
			PrintMessage(Message::mkInfo, "Need more %i par-block(s) for %s", missingblockcount, m_infoName.c_str());
		}

		bool hasMorePars;
		{
			std::lock_guard guard{ m_queuedParFilesMtx };
			hasMorePars = !m_queuedParFiles.empty();
		}

		if (!hasMorePars)
		{
			int blockFound = 0;
			bool requested = RequestMorePars(missingblockcount, &blockFound);
			if (requested)
			{
				m_progressLabel = "Awaiting additional par-files";
				m_fileProgress = 0;
				UpdateProgress();
			}

			{
				std::lock_guard guard{ m_queuedParFilesMtx };
				hasMorePars = !m_queuedParFiles.empty();
				m_queuedParFilesChanged = false;
			}

			int waitAction = nzbget_rs_srp_parchecker_should_wait_for_queued_pars(
				requested ? 1 : 0, hasMorePars ? 1 : 0);
			if (waitAction == 0)
			{
				NzbgetRsBuf errBuf = nzbget_rs_pup_parchecker_format_not_enough_blocks_errmsg(
					missingblockcount, blockFound);
				m_errMsg = errBuf.data ? std::string(errBuf.data, errBuf.len) : std::string();
				nzbget_rs_free(errBuf);
				break;
			}

			if (waitAction == 1)
			{
				// wait until new files are added by "AddParFile" or a change is signaled by "QueueChanged"
				bool queuedParFilesChanged = false;
				while (!queuedParFilesChanged && !IsStopped())
				{
					{
						std::lock_guard guard{ m_queuedParFilesMtx };
						queuedParFilesChanged = m_queuedParFilesChanged;
					}
					Util::Sleep(100);
				}
			}
		}

		if (IsStopped())
		{
			break;
		}

		moreFilesLoaded = LoadMorePars();
		if (moreFilesLoaded)
		{
			GetRepairer()->UpdateVerificationResults();
			res = GetRepairer()->Process(false);
		}
	}

	return res;
}

int ParChecker::ProcessTwinSet()
{
	std::string index = RequestTwinIndex();
	if (index.empty() || IsStopped())
	{
		return Par2::eRepairNotPossible;
	}

	// a set of its own (its packets carry another set id): verified anew, with
	// the files on disk found by the names in its FileDesc packets
	std::string ownParFilename = m_parFilename;
	std::string ownErrMsg = m_errMsg;
	m_parFilename = index;
	m_errMsg.clear();
	Par2::Result res = static_cast<Par2::Result>(PreProcessPar());
	if (res != Par2::eSuccess || IsStopped())
	{
		m_parFilename = ownParFilename;
		m_errMsg = ownErrMsg;
		return Par2::eRepairNotPossible;
	}

	res = GetRepairer()->Process(false);
	if (nzbget_rs_src_parchecker_should_process_twin_missing_blocks(
		(int)res, (int)Par2::eRepairNotPossible,
		(res == Par2::eRepairNotPossible && IsStopped()) ? 1 : 0))
	{
		int blockNeeded = nzbget_rs_psp_parchecker_calc_twin_blocks_needed(
			GetRepairer()->missingblockcount, GetRepairer()->recoverypacketmap.size());
		PrintMessage(Message::mkInfo, "Need %i par-block(s) of the twin's set for %s", blockNeeded, m_infoName.c_str());
		std::vector<std::string> volumes = RequestTwinVolumes(blockNeeded);
		for (const std::string& volume : volumes)
		{
			if (GetRepairer()->LoadPacketsFromFile(volume.c_str()))
			{
				PrintMessage(Message::mkInfo, "File %s of the twin loaded for par-check", FileSystem::BaseFileName(volume.c_str()));
			}
		}
		if (!volumes.empty() && !IsStopped())
		{
			GetRepairer()->UpdateVerificationResults();
			res = GetRepairer()->Process(false);
		}
	}

	int twinErrAction = nzbget_rs_dps_parchecker_classify_twin_repair_err(
		(int)res, (int)Par2::eRepairNotPossible, ownErrMsg.empty() ? 1 : 0);
	NzbgetRsBuf twinErrBuf = nzbget_rs_tpr_parchecker_format_twin_err_msg(
		twinErrAction, ownErrMsg.c_str());
	if (twinErrBuf.data)
	{
		m_errMsg.assign(twinErrBuf.data, twinErrBuf.len);
		nzbget_rs_free(twinErrBuf);
	}
	return res;
}

bool ParChecker::LoadMorePars()
{
	FileList moreFiles;
	{
		std::lock_guard guard{ m_queuedParFilesMtx };
		moreFiles = std::move(m_queuedParFiles);
		m_queuedParFiles.clear();
	}

	for (std::string& parFilename : moreFiles)
	{
		bool loadedOK = GetRepairer()->LoadPacketsFromFile(parFilename.c_str());
		if (loadedOK)
		{
			PrintMessage(Message::mkInfo, "File %s successfully loaded for par-check", FileSystem::BaseFileName(parFilename.c_str()));
		}
		else
		{
			PrintMessage(Message::mkInfo, "Could not load file %s for par-check", FileSystem::BaseFileName(parFilename.c_str()));
		}
	}

	return !moreFiles.empty();
}

void ParChecker::AddParFile(const char * parFilename)
{
	std::lock_guard guard{ m_queuedParFilesMtx };
	m_queuedParFiles.push_back(parFilename);
	m_queuedParFilesChanged = true;
}

void ParChecker::QueueChanged()
{
	std::lock_guard guard{ m_queuedParFilesMtx };
	m_queuedParFilesChanged = true;
}

bool ParChecker::AddSplittedFragments()
{
	std::vector<std::string> extrafiles;

	DirBrowser dir(m_destDir.c_str());
	while (const char* filename = dir.Next())
	{
		if (!IsParredFile(filename) && !IsProcessedFile(filename))
		{
			for (Par2::Par2RepairerSourceFile *sourcefile : GetRepairer()->sourcefiles)
			{
				std::string target = sourcefile->TargetFileName();
				const char* current = FileSystem::BaseFileName(target.c_str());

				// if file was renamed by par-renamer we also check the original filename
				const char* original = FindFileOrigname(current);

				if (MaybeSplittedFragement(filename, current) ||
					(nzbget_rs_psp_parchecker_should_check_original_splitted_fragment(original, current) != 0 &&
					MaybeSplittedFragement(filename, original)))
				{
					detail("Found splitted fragment %s", filename);
					NzbgetRsBuf fragBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
						m_destDir.c_str(), PATH_SEPARATOR, filename);
					extrafiles.emplace_back(fragBuf.data ? fragBuf.data : "", fragBuf.len);
					nzbget_rs_free(fragBuf);
					break;
				}
			}
		}
	}

	bool fragmentsAdded = false;

	if (!extrafiles.empty())
	{
		m_extraFiles += extrafiles.size();
		m_verifyingExtraFiles = true;
		PrintMessage(Message::mkInfo, "Found %i splitted fragments for %s", (int)extrafiles.size(), m_infoName.c_str());
		fragmentsAdded = GetRepairer()->VerifyExtraFiles(extrafiles, m_destDir, false);
		GetRepairer()->UpdateVerificationResults();
		m_verifyingExtraFiles = false;
	}

	return fragmentsAdded;
}

bool ParChecker::MaybeSplittedFragement(const char* filename1, const char* filename2)
{
	return nzbget_rs_parchecker_maybe_splitted_fragment(filename1, filename2) != 0;
}

bool ParChecker::AddMissingFiles()
{
	return AddExtraFiles(true, false, m_destDir.c_str());
}

bool ParChecker::AddDupeFiles()
{
	// the folder of the par2 file, not the file itself (which is no folder to scan)
	BString<1024> directory;
	size_t dirLen = nzbget_rs_tpr_parchecker_strip_last_path_sep_len(
		m_parFilename.c_str(), PATH_SEPARATOR);
	directory.Set(m_parFilename.c_str(), (int)dirLen);

	bool added = AddExtraFiles(false, false, directory);

	if (GetRepairer()->missingblockcount > 0)
	{
		// scanning directories of duplicates
		RequestDupeSources(&m_dupeSources);

		if (!m_dupeSources.empty())
		{
			uint32_t wasBlocksMissing = GetRepairer()->missingblockcount;
			uint32_t totalExtraBlocksFound = 0;
			for (DupeSource& dupeSource : m_dupeSources)
			{
				if (GetRepairer()->missingblockcount > 0 && FileSystem::DirectoryExists(dupeSource.GetDirectory()))
				{
					uint32_t wasBlocksMissing2 = GetRepairer()->missingblockcount;
					bool oneAdded = AddExtraFiles(false, true, dupeSource.GetDirectory());
					added |= oneAdded;
					uint32_t blocksMissing2 = GetRepairer()->missingblockcount;
					dupeSource.SetUsedBlocks(nzbget_rs_pnd_parchecker_calc_dupe_blocks_delta(
						dupeSource.GetUsedBlocks(), wasBlocksMissing2, blocksMissing2,
						wasBlocksMissing, &totalExtraBlocksFound));
				}
			}

			if (totalExtraBlocksFound > 0)
			{
				PrintMessage(Message::mkInfo, "Found extra %u blocks in dupe sources", totalExtraBlocksFound);
			}
			else
			{
				PrintMessage(Message::mkInfo, "No extra blocks found in dupe sources");
			}
		}
	}

	return added;
}

/*
* Files with the same name as in par-file (and a differnt extension) are
* placed at the top of the list to be scanned first.
*/
void ParChecker::SortExtraFiles(std::vector<std::string>& extrafiles)
{
	NzbgetRsBuf stemBuf = nzbget_rs_pup_parchecker_extract_base_par_stem(m_parFilename.c_str());
	std::string baseParFilename = stemBuf.data ? std::string(stemBuf.data, stemBuf.len) : std::string();
	nzbget_rs_free(stemBuf);

	std::sort(
		begin(extrafiles),
		end(extrafiles),
		[&baseParFilename](const std::string& a, const std::string& b)
		{
			return nzbget_rs_dur_parchecker_compare_extra_file_stems(
				FileSystem::BaseFileName(a.c_str()),
				FileSystem::BaseFileName(b.c_str()),
				baseParFilename.c_str()) != 0;
		}
	);
}

// (the list by reference: taken by value, every file found was thrown away, and the
// extended and dupe par-scans never added a block)
void ParChecker::FindExtraFiles(std::vector<std::string>& extrafiles, const char* directory, bool externalDir)
{
	DirBrowser dir(directory);
	while (const char* filename = dir.Next())
	{
		NzbgetRsBuf fullFnBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
			directory, PATH_SEPARATOR, filename);
		std::string fullFilenameStr(fullFnBuf.data ? fullFnBuf.data : "", fullFnBuf.len);
		BString<1024> fullFilename;
		fullFilename.Set(fullFnBuf.data ? fullFnBuf.data : "", (int)fullFnBuf.len);
		nzbget_rs_free(fullFnBuf);

		if (FileSystem::DirectoryExists(fullFilename))
		{
			FindExtraFiles(extrafiles, fullFilename, externalDir);
		}
		else
		{
			bool isParred = !externalDir && IsParredFile(filename);
			bool isProcessed = !externalDir && !isParred && IsProcessedFile(filename);
			if (nzbget_rs_srp_parchecker_should_add_extra_file_entry(
				externalDir ? 1 : 0, isParred ? 1 : 0, isProcessed ? 1 : 0))
			{
				extrafiles.push_back(std::move(fullFilenameStr));
			}
		}
	}
}

bool ParChecker::AddExtraFiles(bool onlyMissing, bool externalDir, const char* directory)
{
	if (externalDir)
	{
		PrintMessage(Message::mkInfo, "Performing dupe par-scan for %s in %s", m_infoName.c_str(), FileSystem::BaseFileName(directory));
	}
	else
	{
		PrintMessage(Message::mkInfo, "Performing extra par-scan for %s", m_infoName.c_str());
	}

	std::vector<std::string> extrafiles;

	FindExtraFiles(extrafiles, directory, externalDir);

	SortExtraFiles(extrafiles);

	// Scan files
	bool filesAdded = false;
	if (!extrafiles.empty())
	{
		m_extraFiles += extrafiles.size();
		m_verifyingExtraFiles = true;

		// adding files one by one until all missing files are found
		size_t idx = 0;
		while (!IsStopped() && idx < extrafiles.size())
		{
			const std::string& extraFile = extrafiles[idx];
			++idx;

			uint32_t wasFilesMissing = GetRepairer()->missingfilecount;
			uint32_t wasBlocksMissing = GetRepairer()->missingblockcount;

			GetRepairer()->VerifyExtraFiles({ extraFile }, m_destDir, false);
			GetRepairer()->UpdateVerificationResults();

			int fileAdded = 0;
			int blockAdded = 0;
			uint32_t blocksDelta = 0;
			int registerParred = 0;
			filesAdded = nzbget_rs_pnd_parchecker_eval_extra_file_step(
				wasFilesMissing, GetRepairer()->missingfilecount,
				wasBlocksMissing, GetRepairer()->missingblockcount,
				externalDir ? 1 : 0, filesAdded ? 1 : 0,
				&fileAdded, &blockAdded, &blocksDelta, &registerParred) != 0;
			if (registerParred != 0)
			{
				PrintMessage(Message::mkInfo, "Found missing file %s", FileSystem::BaseFileName(extraFile.c_str()));
				RegisterParredFile(FileSystem::BaseFileName(extraFile.c_str()));
			}
			else if (blockAdded != 0)
			{
				PrintMessage(Message::mkInfo, "Found %u missing blocks", blocksDelta);
			}

			int abortReason = nzbget_rs_pup_parchecker_should_abort_extra_scan(
				onlyMissing ? 1 : 0, GetRepairer()->missingfilecount, GetRepairer()->missingblockcount);
			if (abortReason == 1)
			{
				PrintMessage(Message::mkInfo, "All missing files found, aborting par-scan");
				break;
			}
			if (abortReason == 2)
			{
				PrintMessage(Message::mkInfo, "All missing blocks found, aborting par-scan");
				break;
			}
		}

		m_verifyingExtraFiles = false;
	}

	return filesAdded;
}

bool ParChecker::IsProcessedFile(const char* filename)
{
	for (std::string& processedFilename : m_processedFiles)
	{
		if (nzbget_rs_dqh_bytes_eq_ignore_case(FileSystem::BaseFileName(processedFilename.c_str()), filename) != 0)
		{
			return true;
		}
	}

	return false;
}

void ParChecker::SignalFilename(std::string str)
{
	std::lock_guard guard{ m_signalMtx };

	if (nzbget_rs_sap_parchecker_should_mark_verifying_repaired((int)m_stage, (int)ptRepairing))
	{
		m_stage = ptVerifyingRepaired;
	}

	// don't print progress messages when verifying repaired files in quick verification mode,
	// because repaired files are not verified in this mode
	const char* stageMsg = nzbget_rs_dqh_parchecker_stage_message((int)m_stage);
	if (nzbget_rs_pup_parchecker_should_print_stage_msg(
		(int)m_stage, (int)ptVerifyingRepaired, m_parQuick ? 1 : 0) != 0)
	{
		PrintMessage(Message::mkInfo, "%s %s", stageMsg, str.c_str());
	}

	NzbgetRsBuf progBuf = nzbget_rs_dqh_parchecker_format_action_label(
		stageMsg, str.c_str());
	m_progressLabel.assign(progBuf.data ? progBuf.data : "", progBuf.len);
	nzbget_rs_free(progBuf);

	if (nzbget_rs_sap_parchecker_should_record_processed_file(
		(int)m_stage, (int)ptLoadingPars, (int)ptVerifyingSources))
	{
		m_processedFiles.push_back(std::move(str));
	}

	m_fileProgress = 0;
	UpdateProgress();
}

void ParChecker::SignalProgress(int progress)
{
	std::lock_guard guard{ m_signalMtx };

	m_fileProgress = progress;

	int sourceFilesCount = nzbget_rs_dqh_parchecker_effective_source_files_count(
		(int)m_stage, (int)ptRepairing, (int)ptVerifyingRepaired,
		GetRepairer()->sourcefiles.size());
	m_stageProgress = nzbget_rs_parchecker_calc_stage_progress(
		(int)m_stage, m_fileProgress, m_processedCount, m_filesToRepair,
		sourceFilesCount, m_extraFiles, m_quickFiles);
	debug("Current-progress: %i, Total-progress: %i", m_fileProgress, m_stageProgress);

	UpdateProgress();
}

void ParChecker::SignalDone(std::string fileName, int available, int total)
{
	std::lock_guard guard{ m_signalMtx };

	++m_processedCount;

	bool ignore = nzbget_rs_dqh_parchecker_should_check_ignore_ext(
		(int)m_stage, (int)ptVerifyingSources, available, total,
		m_verifyingExtraFiles ? 1 : 0) != 0
		? Util::MatchFileExt(fileName.c_str(), g_Options->GetParIgnoreExt(), ",;")
		: false;
	int newHasDamaged = m_hasDamagedFiles ? 1 : 0;
	int badBlocks = 0;
	if (nzbget_rs_pnd_parchecker_eval_signal_done(
		(int)m_stage, (int)ptVerifyingSources, available, total,
		m_verifyingExtraFiles ? 1 : 0, ignore ? 1 : 0,
		m_hasDamagedFiles ? 1 : 0, &newHasDamaged, &badBlocks) != 0)
	{
		m_hasDamagedFiles = newHasDamaged != 0;
		bool fileExists = true;
		for (Par2::Par2RepairerSourceFile* sourcefile : GetRepairer()->sourcefiles)
		{
			if (!sourcefile) 
			{
				continue;
			}

			std::string targetFileName = sourcefile->TargetFileName();
			if (!sourcefile->GetTargetExists() && fileName == FileSystem::BaseFileName(targetFileName.c_str()))
			{
				fileExists = false;
				break;
			}
		}

		if (fileExists)
		{
			PrintMessage(Message::mkWarning, "File %s has %i bad block(s) of total %i block(s)%s",
				fileName.c_str(), badBlocks, total, nzbget_rs_dqh_parchecker_ignore_suffix(ignore ? 1 : 0));
		}
		else
		{
			PrintMessage(Message::mkWarning, "File %s with %i block(s) is missing%s",
				fileName.c_str(), total, nzbget_rs_dqh_parchecker_ignore_suffix(ignore ? 1 : 0));
		}

		if (!IsProcessedFile(fileName.c_str()))
		{
			m_processedFiles.push_back(std::move(fileName));
		}
	}
}

/*
 * Only if ParQuick isn't enabled:
 * For empty damaged files the callback-function "signal_done" isn't called and the flag "m_bHasDamagedFiles"
 * therefore isn't set. In this function we expicitly check such files.
 */
void ParChecker::CheckEmptyFiles()
{
	for (Par2::Par2RepairerSourceFile* sourcefile : GetRepairer()->sourcefiles)
	{
		if (sourcefile && sourcefile->GetDescriptionPacket())
		{
			// GetDescriptionPacket()->FileName() returns a temp string object, which we need to hold for a while
			std::string filenameObj = Par2::DescriptionPacket::TranslateFilenameFromPar2ToLocal(
				m_parCout, 
				m_parCerr, 
				Par2::nlNormal,
				sourcefile->GetDescriptionPacket()->FileName()
			);
			if (!filenameObj.empty() && !IsProcessedFile(filenameObj.c_str()))
			{
				bool ignore = Util::MatchFileExt(filenameObj.c_str(), g_Options->GetParIgnoreExt(), ",;");
				m_hasDamagedFiles = nzbget_rs_dps_parchecker_update_damaged_flag(
					m_hasDamagedFiles ? 1 : 0, ignore ? 1 : 0) != 0;

				uint32_t total = sourcefile->GetVerificationPacket() ? sourcefile->GetVerificationPacket()->BlockCount() : 0;
				PrintMessage(Message::mkWarning, "File %s has %u bad block(s) of total %u block(s)%s",
					filenameObj.c_str(), total, total, nzbget_rs_dqh_parchecker_ignore_suffix(ignore ? 1 : 0));
			}
		}
		else
		{
			m_hasDamagedFiles = true;
		}
	}
}

void ParChecker::Cancel()
{
	{
		std::lock_guard guard{ m_repairerMtx };
		if (m_repairer)
		{
			m_repairer->GetRepairer()->cancelled = true;
		}
	}
	QueueChanged();
}

void ParChecker::SaveSourceList()
{
	// Buliding a list of DiskFile-objects, marked as source-files

	for (Par2::Par2RepairerSourceFile* sourcefile : GetRepairer()->sourcefiles)
	{
		if (!sourcefile)
		{
			continue;
		}

		std::vector<Par2::DataBlock>::iterator it2 = sourcefile->SourceBlocks();
		for (uint32_t i = 0; i < sourcefile->BlockCount(); ++i, ++it2)
		{
			Par2::DataBlock& block = *it2;
			Par2::DiskFile* sourceFile = block.GetDiskFile();
			if (sourceFile &&
				std::find(m_sourceFiles.begin(), m_sourceFiles.end(), sourceFile) == m_sourceFiles.end())
			{
				m_sourceFiles.push_back(sourceFile);
			}
		}
	}
}

void ParChecker::DeleteLeftovers()
{
	// After repairing check if all DiskFile-objects saved by "SaveSourceList()" have
	// corresponding target-files. If not - the source file was replaced. In this case
	// the DiskFile-object points to the renamed bak-file, which we can delete.

	for (Par2::DiskFile* sourceFile : m_sourceFiles)
	{
		if (!sourceFile)
		{
			continue;
		}

		bool found = false;
		for (Par2::Par2RepairerSourceFile* sourcefile : GetRepairer()->sourcefiles)
		{
			if (sourcefile && sourcefile->GetTargetFile() == sourceFile)
			{
				found = true;
				break;
			}
		}

		if (!found)
		{
			std::string fileName = sourceFile->FileName().c_str();
			PrintMessage(Message::mkInfo, "Deleting file %s", FileSystem::BaseFileName(fileName.c_str()));
			FileSystem::DeleteFile(fileName.c_str());
		}
	}
}

/**
 * This function implements quick par verification replacing the standard verification routine
 * from libpar2:
 * - for successfully downloaded files the function compares CRC of the file computed during
 *   download with CRC stored in PAR2-file;
 * - for partially downloaded files the CRCs of articles are compared with block-CRCs stored
 *   in PAR2-file;
 * - for completely failed files (not a single successful article) no verification is needed at all.
 *
 * Limitation of the function:
 * This function requires every block in the file to have an unique CRC (across all blocks
 * of the par-set). Otherwise the full verification is performed.
 * The limitation can be avoided by using something more smart than "verificationhashtable.Lookup"
 * but in the real life all blocks have unique CRCs and the simple "Lookup" works good enough.
 */
ParChecker::EFileStatus ParChecker::VerifyDataFile(Par2::DiskFile& diskFile, Par2::Par2RepairerSourceFile& sourceFile, int& availableBlocks)
{
	Par2::VerificationPacket* packet = (m_stage == ptVerifyingSources && sourceFile.GetTargetExists())
		? sourceFile.GetVerificationPacket()
		: nullptr;
	std::string filename = (packet && sourceFile.GetTargetFile())
		? sourceFile.GetTargetFile()->FileName()
		: std::string();
	int64 fileSize = (!filename.empty()) ? FileSystem::FileSize(filename.c_str()) : -1;
	int precond = nzbget_rs_pnd_parchecker_check_verify_data_file_preconditions(
		(int)m_stage, (int)ptVerifyingSources, sourceFile.GetTargetExists() ? 1 : 0,
		packet ? 1 : 0, fileSize, sourceFile.BlockCount());
	if (precond == 1)
	{
		return fsSuccess;
	}
	if (precond == 2)
	{
		return fsUnknown;
	}
	if (precond == 3)
	{
		availableBlocks = 0;
		return fsFailure;
	}

	// find file status and CRC computed during download
	uint32 downloadCrc;
	SegmentList segments;
	EFileStatus	fileStatus = FindFileCrc(FileSystem::BaseFileName(filename.c_str()), &downloadCrc, &segments);
	ValidBlocks validBlocks;

	if (fileStatus == fsFailure || fileStatus == fsUnknown)
	{
		return fileStatus;
	}
	else if ((fileStatus == fsSuccess && !VerifySuccessDataFile(diskFile, sourceFile, downloadCrc)) ||
		(fileStatus == fsPartial && !VerifyPartialDataFile(diskFile, sourceFile, segments, validBlocks)))
	{
		PrintMessage(Message::mkWarning, "Quick verification failed for %s file %s, performing full verification instead",
			nzbget_rs_dqh_parchecker_file_quality_label((int)fileStatus, (int)fsSuccess),
			FileSystem::BaseFileName(filename.c_str()));
		return fsUnknown; // let libpar2 do the full verification of the file
	}

	// attach verification blocks to the file
	availableBlocks = 0;
	Par2::u64 blocksize = GetRepairer()->mainpacket->BlockSize();
	std::deque<const Par2::VerificationHashEntry*> undoList;
	for (uint32 i = 0; i < packet->BlockCount(); i++)
	{
		if (fileStatus == fsSuccess || validBlocks.at(i))
		{
			const Par2::FILEVERIFICATIONENTRY* entry = packet->VerificationEntry(i);
			Par2::u32 blockCrc = entry->crc;

			// Look for a match
			const Par2::VerificationHashEntry* hashEntry = GetRepairer()->verificationhashtable.Lookup(blockCrc);
			if (nzbget_rs_dps_parchecker_is_verify_hash_mismatch(
				hashEntry ? 1 : 0,
				(hashEntry && hashEntry->SourceFile() == &sourceFile) ? 1 : 0,
				(hashEntry && hashEntry->IsSet()) ? 1 : 0) != 0)
			{
				// no match found, revert back the changes made by "pHashEntry->SetBlock"
				for (const Par2::VerificationHashEntry* undoEntry : undoList)
				{
					undoEntry->SetBlock(nullptr, 0);
				}
				return fsUnknown;
			}

			undoList.push_back(hashEntry);
			hashEntry->SetBlock(&diskFile, nzbget_rs_dps_parchecker_calc_block_offset(i, blocksize));
			++availableBlocks;
		}
	}

	m_quickFiles++;
	PrintMessage(Message::mkDetail, "Quickly verified %s file %s",
		nzbget_rs_dqh_parchecker_file_quality_label((int)fileStatus, (int)fsSuccess),
		FileSystem::BaseFileName(filename.c_str()));

	return fileStatus;
}

bool ParChecker::VerifySuccessDataFile(Par2::DiskFile&, Par2::Par2RepairerSourceFile& sourceFile, uint32 downloadCrc)
{
	Par2::u64 blocksize = GetRepairer()->mainpacket->BlockSize();
	Par2::VerificationPacket* packet = sourceFile.GetVerificationPacket();

	// extend lDownloadCrc to block size
	uint64_t padBytes = nzbget_rs_dur_parchecker_calc_pad_bytes(
		blocksize, packet->BlockCount(), sourceFile.GetTargetFile()->FileSize());
	downloadCrc = Par2::CRCUpdateBlock(downloadCrc ^ 0xFFFFFFFF, padBytes) ^ 0xFFFFFFFF;
	debug("Download-CRC: %.8x", downloadCrc);

	// compute file CRC using CRCs of blocks
	std::vector<uint32_t> blockCrcs;
	blockCrcs.reserve(packet->BlockCount());
	for (uint32 i = 0; i < packet->BlockCount(); i++)
	{
		const Par2::FILEVERIFICATIONENTRY* entry = packet->VerificationEntry(i);
		blockCrcs.push_back(entry->crc);
	}
	uint32 parCrc = nzbget_rs_pup_parchecker_combine_block_crcs(
		blockCrcs.data(), blockCrcs.size(), blocksize);
	std::string fileName = sourceFile.GetTargetFile()->FileName();
	debug("Block-CRC: %x, filename: %s", parCrc, FileSystem::BaseFileName(fileName.c_str()));

	return parCrc == downloadCrc;
}

bool ParChecker::VerifyPartialDataFile(Par2::DiskFile&, Par2::Par2RepairerSourceFile& sourceFile, SegmentList& segments, ValidBlocks& validBlocks)
{
	Par2::VerificationPacket* packet = sourceFile.GetVerificationPacket();
	int64 blocksize = GetRepairer()->mainpacket->BlockSize();
	std::string filenameObj = sourceFile.GetTargetFile()->FileName();
	const char* filename = filenameObj.c_str();
	int64 fileSize = sourceFile.GetTargetFile()->FileSize();

	// determine presumably valid and bad blocks based on article download status
	validBlocks.resize(packet->BlockCount(), false);
	if (!validBlocks.empty())
	{
		std::vector<NzbgetRsParSegment> rsSegs;
		rsSegs.reserve(segments.size());
		for (Segment& segment : segments)
		{
			rsSegs.push_back({segment.GetSuccess() ? 1 : 0, segment.GetOffset(), segment.GetSize(), segment.GetCrc()});
		}
		std::vector<uint8_t> outValid(validBlocks.size(), 0);
		nzbget_rs_parchecker_compute_valid_blocks(
			validBlocks.size(), blocksize, fileSize, rsSegs.data(), rsSegs.size(), outValid.data());
		for (size_t i = 0; i < validBlocks.size(); ++i)
		{
			validBlocks[i] = outValid[i] != 0;
		}
	}

	DiskFile infile;
	if (!infile.Open(filename, DiskFile::omRead))
	{
		PrintMessage(Message::mkError, "Could not open file %s: %s",
			filename, *FileSystem::GetLastErrorMessage());
	}

	// For each sequential range of presumably valid blocks:
	// - compute par-CRC of the range of blocks using block CRCs;
	// - compute download-CRC for the same byte range using CRCs of articles; if articles and block
	//   overlap - read a little bit of data from the file and calculate its CRC;
	// - compare two CRCs - they must match; if not - the file is more damaged than we thought -
	//   let libpar2 do the full verification of the file in this case.
	std::vector<uint8_t> validBlockFlags(validBlocks.size(), 0);
	std::vector<uint32_t> blockCrcs(validBlocks.size(), 0);
	for (size_t i = 0; i < validBlocks.size(); ++i)
	{
		validBlockFlags[i] = validBlocks[i] ? 1 : 0;
		blockCrcs[i] = packet->VerificationEntry((uint32)i)->crc;
	}
	size_t searchIdx = 0;
	size_t runStart = 0, runEnd = 0;
	while (nzbget_rs_tpr_parchecker_next_valid_block_run(
		validBlockFlags.empty() ? nullptr : validBlockFlags.data(),
		validBlockFlags.size(),
		searchIdx,
		&runStart,
		&runEnd) != 0)
	{
		uint32 parCrc = nzbget_rs_pup_parchecker_combine_block_crcs(
			&blockCrcs[runStart], runEnd - runStart + 1, (uint64_t)blocksize);
		int blockStart = (int)runStart;
		int blockEnd = (int)runEnd;
		int64 bytesStart = 0;
		int64 bytesEnd = 0;
		int64 clampedEnd = 0;
		size_t padLen = 0;
		nzbget_rs_psp_parchecker_calc_block_byte_range(
			blockStart, blockEnd, blocksize, fileSize,
			&bytesStart, &bytesEnd, &clampedEnd, &padLen);
		uint32 downloadCrc = 0;
		bool ok = SmartCalcFileRangeCrc(infile, bytesStart, clampedEnd, segments, downloadCrc);
		if (ok && padLen > 0)
		{
			downloadCrc = Par2::CRCUpdateBlock(downloadCrc ^ 0xFFFFFFFF, padLen) ^ 0xFFFFFFFF;
		}

		if (!ok || downloadCrc != parCrc)
		{
			infile.Close();
			return false;
		}
		searchIdx = runEnd + 1;
	}

	infile.Close();

	return true;
}

/*
 * Compute CRC of bytes range of file using CRCs of segments and reading some data directly
 * from file if necessary
 */
bool ParChecker::SmartCalcFileRangeCrc(DiskFile& file, int64 start, int64 end, SegmentList& segments,
	uint32& downloadCrcOut)
{
	std::vector<NzbgetRsParSegment> rsSegs;
	rsSegs.reserve(segments.size());
	for (Segment& segment : segments)
	{
		rsSegs.push_back({segment.GetSuccess() ? 1 : 0, segment.GetOffset(), segment.GetSize(), segment.GetCrc()});
	}
	struct DumbCtx
	{
		ParChecker* self;
		DiskFile* file;
	} ctx{this, &file};
	auto dumbCb = [](void* rawCtx, int64_t s, int64_t e, uint32_t* outCrc) -> int {
		auto* c = static_cast<DumbCtx*>(rawCtx);
		uint32 crc = 0;
		if (!c->self->DumbCalcFileRangeCrc(*c->file, s, e, crc))
		{
			return 0;
		}
		*outCrc = crc;
		return 1;
	};
	uint32 outCrc = 0;
	if (nzbget_rs_parchecker_smart_calc_range_crc(
		start, end, rsSegs.data(), rsSegs.size(), &ctx, dumbCb, &outCrc) == 0)
	{
		return false;
	}
	downloadCrcOut = outCrc;
	return true;
}

/*
 * Compute CRC of bytes range of file reading the data directly from file
 */
bool ParChecker::DumbCalcFileRangeCrc(DiskFile& file, int64 start, int64 end, uint32& downloadCrcOut)
{
	if (!file.Seek(start))
	{
		return false;
	}

	CharBuffer buffer(1024 * 64);
	Crc32 downloadCrc;

	int cnt = buffer.Size();
	while (int needBytes = nzbget_rs_pnd_parchecker_calc_dumb_crc_read_size(
		cnt, buffer.Size(), start, end))
	{
		cnt = (int)file.Read(buffer, needBytes);
		downloadCrc.Append((uchar*)(char*)buffer, cnt);
		start += cnt;
	}

	downloadCrcOut = downloadCrc.Finish();
	return true;
}

std::string ParChecker::GetPacketCreator()
{
	Par2::CREATORPACKET* creatorpacket;
	if (GetRepairer()->creatorpacket &&
		(creatorpacket = (Par2::CREATORPACKET*)(((RepairCreatorPacket*)GetRepairer()->creatorpacket)->packetdata)))
	{
		size_t len = nzbget_rs_psp_parchecker_calc_creator_packet_payload_len(
			creatorpacket->header.length, sizeof(Par2::PACKET_HEADER));
		if (len > 0)
		{
			return std::string((const char*)creatorpacket->client);
		}
	}

	return "";
}

int64 ParChecker::GetBlockSize()
{
	if (!m_repairer || !GetRepairer() || !GetRepairer()->mainpacket)
	{
		return 0;
	}
	return (int64)GetRepairer()->mainpacket->BlockSize();
}

#endif
