/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2007-2019 Andrey Prygunkov <hugbug@users.sourceforge.net>
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


#ifndef PARCHECKER_H
#define PARCHECKER_H

#ifndef DISABLE_PARCHECK

#include <atomic>
#include <deque>
#include <memory>
#include <mutex>
#include <ostream>
#include <streambuf>
#include <string>
#include <vector>

#include "FileSystem.h"
#include "Log.h"

class Repairer;

class ParChecker
{
public:
	virtual ~ParChecker() = default;

	enum EStatus
	{
		psFailed,
		psRepairPossible,
		psRepaired,
		psRepairNotNeeded
	};

	enum EStage
	{
		ptLoadingPars,
		ptVerifyingSources,
		ptRepairing,
		ptVerifyingRepaired,
	};

	class AbstractRepairer
	{
	public:
		virtual ~AbstractRepairer() = default;
		virtual Repairer* GetRepairer() = 0;
	};

	void Execute();
	void SetDestDir(const char* destDir) { m_destDir = destDir ? destDir : ""; }
	const char* GetDestDir() { return m_destDir.c_str(); }
	const char* GetParFilename() { return m_parFilename.c_str(); }
	const char* GetInfoName() { return m_infoName.c_str(); }
	void SetInfoName(const char* infoName) { m_infoName = infoName ? infoName : ""; }
	void SetNzbName(const char* nzbName) { m_nzbName = nzbName ? nzbName : ""; }
	void SetParQuick(bool parQuick) { m_parQuick = parQuick; }
	bool GetParQuick() { return m_parQuick; }
	void SetForceRepair(bool forceRepair) { m_forceRepair = forceRepair; }
	bool GetForceRepair() { return m_forceRepair; }
	void SetParFull(bool parFull) { m_parFull = parFull; }
	bool GetParFull() { return m_parFull; }
	EStatus GetStatus() { return m_status; }
	void AddParFile(const char* parFilename);
	void QueueChanged();
	void Cancel();

protected:
	class Segment
	{
	public:
		Segment(bool success, int64 offset, int size, uint32 crc) 
			: m_success(success)
			, m_offset(offset)
			, m_size(size)
			, m_crc(crc) 
		{}
		bool GetSuccess() { return m_success; }
		int64 GetOffset() { return m_offset; }
		int GetSize() { return m_size; }
		uint32 GetCrc() { return m_crc; }

	private:
		bool m_success;
		int64 m_offset;
		int m_size;
		uint32 m_crc;
	};

	typedef std::deque<Segment> SegmentList;

	class DupeSource
	{
	public:
		DupeSource(int id, const char* directory) 
			: m_id(id)
			, m_directory(directory ? directory : "") 
		{}
		int GetId() { return m_id; }
		const char* GetDirectory() { return m_directory.c_str(); }
		int GetUsedBlocks() { return m_usedBlocks; }
		void SetUsedBlocks(int usedBlocks) { m_usedBlocks = usedBlocks; }

	private:
		int m_id;
		int m_usedBlocks = 0;
		std::string m_directory;
	};

	typedef std::deque<DupeSource> DupeSourceList;

	enum EFileStatus
	{
		fsUnknown,
		fsSuccess,
		fsPartial,
		fsFailure
	};

	/**
	* Unpause par2-files
	* returns true, if the files with required number of blocks were unpaused,
	* or false if there are no more files in queue for this collection or not enough blocks
	*/
	virtual bool RequestMorePars(int blockNeeded, int* blockFound) = 0;
	virtual void UpdateProgress() {}
	virtual bool IsStopped() { return false; }
	virtual void Completed() {}
	virtual void PrintMessage([[maybe_unused]] Message::EKind kind,
		[[maybe_unused]] const char* format, ...) PRINTF_SYNTAX(3) {}
	virtual void RegisterParredFile([[maybe_unused]] const char* filename) {}
	virtual void RegisterVerifiedFile([[maybe_unused]] const char* filename) {}
	virtual bool IsParredFile([[maybe_unused]] const char* filename) { return false; }
	virtual EFileStatus FindFileCrc([[maybe_unused]] const char* filename, [[maybe_unused]] uint32* crc,
		[[maybe_unused]] SegmentList* segments) { return fsUnknown; }
	virtual const char* FindFileOrigname([[maybe_unused]] const char* filename) { return nullptr; }
	virtual void RequestDupeSources([[maybe_unused]] DupeSourceList* dupeSourceList) {}
	virtual void StatDupeSources([[maybe_unused]] DupeSourceList* dupeSourceList) {}
	/* when its own par2-files can't repair the set: the smallest par2-file of a twin
	 * (a duplicate of byte-identical files, with its own par2 set) fetched into the
	 * destination; its path, or "" for none */
	virtual std::string RequestTwinIndex() { return ""; }
	/* par2-volumes of that twin holding at least <blockNeeded> blocks of its set,
	 * fetched into the destination; their paths */
	virtual std::vector<std::string> RequestTwinVolumes([[maybe_unused]] int blockNeeded) { return {}; }
	EStage GetStage() { return m_stage; }
	const char* GetProgressLabel() { return m_progressLabel.c_str(); }
	int GetFileProgress() { return m_fileProgress; }
	int GetStageProgress() { return m_stageProgress; }

private:
	class StreamBuf final : public std::streambuf
	{
	public:
		StreamBuf(ParChecker* owner, Message::EKind kind) : m_owner(owner), m_kind(kind) {}
		virtual int overflow(int ch) override;
	private:
		std::string m_buffer;
		ParChecker* m_owner;
		Message::EKind m_kind;
	};

	typedef std::deque<std::string> FileList;
	typedef std::vector<bool> ValidBlocks;

	std::string m_infoName;
	std::string m_destDir;
	std::string m_nzbName;
	std::string m_parFilename;
	std::string m_progressLabel;
	std::string m_errMsg;

	int m_processedCount;
	int m_filesToRepair;
	int m_extraFiles;
	int m_fileProgress;
	int m_stageProgress;

	EStatus m_status = psFailed;

	std::atomic<EStage> m_stage{ptLoadingPars};
	std::atomic<int> m_quickFiles{0};

	FileList m_queuedParFiles;
	FileList m_processedFiles;
	std::vector<std::string> m_verifiedFiles;
	DupeSourceList m_dupeSources;
	StreamBuf m_parOutStream{this, Message::mkDetail};
	StreamBuf m_parErrStream{this, Message::mkError};
	std::ostream m_parCout{&m_parOutStream};
	std::ostream m_parCerr{&m_parErrStream};
	std::mutex m_queuedParFilesMtx;
	std::mutex m_repairerMtx;
	std::mutex m_signalMtx;

	// "m_repairer" wraps the Rust par-rs repairer via an abstract class.
	std::unique_ptr<AbstractRepairer> m_repairer;

	std::atomic<bool> m_parQuick{false};
	std::atomic<bool> m_parFull{false};

	bool m_queuedParFilesChanged;
	bool m_verifyingExtraFiles;
	bool m_cancelled;
	bool m_hasDamagedFiles;
	bool m_forceRepair = false;

	Repairer* GetRepairer() { return m_repairer->GetRepairer(); }

public:
	// the par-set's block size once a par2-file is loaded, else 0
	int64 GetBlockSize();
private:

	void Cleanup();
	EStatus RunParCheckAll();
	EStatus RunParCheck(std::string parFilename);
	int PreProcessPar();
	bool LoadMainParBak();
	int ProcessMorePars();
	int ProcessTwinSet();
	bool LoadMorePars();
	bool AddSplittedFragments();
	bool AddMissingFiles();
	bool AddDupeFiles();
	bool AddExtraFiles(bool onlyMissing, bool externalDir, const char* directory);
	bool IsProcessedFile(const char* filename);
	void SaveSourceList();
	void DeleteLeftovers();
	void SignalFilename(std::string str);
	void SignalProgress(int progress);
	void SignalDone(std::string str, int available, int total);
	void FindExtraFiles(std::vector<std::string>& extrafiles, const char* directory, bool externalDir);
	void SortExtraFiles(std::vector<std::string>& extrafiles);
	bool SmartCalcFileRangeCrc(DiskFile& file, int64 start, int64 end, SegmentList& segments, uint32& downloadCrc);
	bool DumbCalcFileRangeCrc(DiskFile& file, int64 start, int64 end, uint32& downloadCrc);
	void CheckEmptyFiles();
	std::string GetPacketCreator();
	bool MaybeSplittedFragement(const char* filename1, const char* filename2);

	friend class Repairer;
};

#endif

#ifndef DISABLE_PARCHECK

#include <vector>
#include <string>
#include <algorithm>

#include "ParParser.h"
#include "Log.h"
#include "Options.h"
#include "Util.h"
#include "FileSystem.h"
#include "nzbget_rs.h"

class Repairer final : public ParChecker::AbstractRepairer
{
public:
	Repairer(ParChecker* owner)
		: m_owner(owner)
	{
		int configuredParBuffer = g_Options->GetParBuffer();
		int monitorMem = g_MemoryMonitor ? g_MemoryMonitor->GetEffectiveParBufferMB(configuredParBuffer) : 0;
		nzbget_rs_parchecker_calc_repairer_resources(
			g_Options->GetParThreads(), Util::NumberOfCpuCores(),
			g_MemoryMonitor != nullptr ? 1 : 0, monitorMem, configuredParBuffer,
			&m_threadsToUse, &m_memToUse);
		m_handle = nzbget_rs_par_repairer_new(
			m_owner->GetDestDir(),
			m_threadsToUse,
			m_memToUse,
			m_owner->GetParQuick() ? 1 : 0,
			g_Options->GetParScan() == Options::psFull ? 1 : 0,
			g_Options->GetParIgnoreExt());
	}

	~Repairer() override
	{
		if (m_handle)
		{
			nzbget_rs_par_repairer_free(m_handle);
			m_handle = nullptr;
		}
	}

	int PreProcess(const std::string& parFilename)
	{
		NzbgetRsParCallbacks cbs = MakeCallbacks();
		return nzbget_rs_par_repairer_preprocess(m_handle, parFilename.c_str(), &cbs);
	}

	int Process(bool dorepair)
	{
		if (dorepair)
		{
			m_owner->PrintMessage(Message::mkInfo, "Using %i thread(s) and %i MB to repair %u block(s) for %s",
				m_threadsToUse, m_memToUse, MissingBlockCount(), m_owner->m_nzbName.c_str());
		}
		NzbgetRsParCallbacks cbs = MakeCallbacks();
		int res = nzbget_rs_par_repairer_process(m_handle, dorepair ? 1 : 0, &cbs);
		debug("ParChecker: Process-result=%i", res);
		return res;
	}

	bool LoadPacketsFromFile(const char* filename)
	{
		NzbgetRsParCallbacks cbs = MakeCallbacks();
		return nzbget_rs_par_repairer_load_extra_par2_file(m_handle, filename, &cbs) != 0;
	}

	bool VerifyExtraFiles(const std::vector<std::string>& extrafiles, bool isSplitFragment)
	{
		NzbgetRsParCallbacks cbs = MakeCallbacks();
		bool anyAdded = false;
		for (const std::string& extra : extrafiles)
		{
			if (nzbget_rs_par_repairer_verify_extra_file(
				m_handle, extra.c_str(), isSplitFragment ? 1 : 0, &cbs) != 0)
			{
				anyAdded = true;
			}
		}
		return anyAdded;
	}

	void Cancel()
	{
		nzbget_rs_par_repairer_cancel(m_handle);
	}

	uint32_t MissingFileCount() const
	{
		return nzbget_rs_par_repairer_missing_file_count(m_handle);
	}

	uint32_t DamagedFileCount() const
	{
		return nzbget_rs_par_repairer_damaged_file_count(m_handle);
	}

	uint32_t MissingBlockCount() const
	{
		return nzbget_rs_par_repairer_missing_block_count(m_handle);
	}

	uint32_t RecoveryBlockCount() const
	{
		return nzbget_rs_par_repairer_recovery_block_count(m_handle);
	}

	int64_t BlockSize() const
	{
		return nzbget_rs_par_repairer_block_size(m_handle);
	}

	std::string Creator() const
	{
		NzbgetRsBuf buf = nzbget_rs_par_repairer_creator(m_handle);
		std::string out(buf.data ? buf.data : "", buf.len);
		nzbget_rs_free(buf);
		return out;
	}

	size_t SourceFileCount() const
	{
		return nzbget_rs_par_repairer_source_file_count(m_handle);
	}

	bool SourceFileInfo(
		size_t idx,
		std::string* targetFilename,
		std::string* targetPath,
		bool* targetExists,
		bool* isCompleteTarget,
		uint32_t* blockCount,
		uint32_t* availableBlocks) const
	{
		NzbgetRsBuf fnBuf{};
		NzbgetRsBuf pathBuf{};
		int exists = 0;
		int completeTarget = 0;
		uint32_t bc = 0;
		uint32_t ab = 0;
		if (nzbget_rs_par_repairer_source_file_info(
			m_handle,
			idx,
			targetFilename ? &fnBuf : nullptr,
			targetPath ? &pathBuf : nullptr,
			targetExists ? &exists : nullptr,
			isCompleteTarget ? &completeTarget : nullptr,
			blockCount ? &bc : nullptr,
			availableBlocks ? &ab : nullptr) == 0)
		{
			return false;
		}
		if (targetFilename)
		{
			targetFilename->assign(fnBuf.data ? fnBuf.data : "", fnBuf.len);
			nzbget_rs_free(fnBuf);
		}
		if (targetPath)
		{
			targetPath->assign(pathBuf.data ? pathBuf.data : "", pathBuf.len);
			nzbget_rs_free(pathBuf);
		}
		if (targetExists)
		{
			*targetExists = exists != 0;
		}
		if (isCompleteTarget)
		{
			*isCompleteTarget = completeTarget != 0;
		}
		if (blockCount)
		{
			*blockCount = bc;
		}
		if (availableBlocks)
		{
			*availableBlocks = ab;
		}
		return true;
	}

	size_t LeftoverCount() const
	{
		return nzbget_rs_par_repairer_leftover_count(m_handle);
	}

	std::string LeftoverPath(size_t idx) const
	{
		NzbgetRsBuf buf = nzbget_rs_par_repairer_leftover_path(m_handle, idx);
		std::string out(buf.data ? buf.data : "", buf.len);
		nzbget_rs_free(buf);
		return out;
	}

	Repairer* GetRepairer() override { return this; }

private:
	NzbgetRsParCallbacks MakeCallbacks()
	{
		NzbgetRsParCallbacks cbs{};
		cbs.ctx = this;
		cbs.on_filename = [](void* ctx, const char* name) {
			auto* self = static_cast<Repairer*>(ctx);
			self->m_owner->SignalFilename(name ? name : "");
		};
		cbs.on_progress = [](void* ctx, int progress) {
			auto* self = static_cast<Repairer*>(ctx);
			self->m_owner->SignalProgress(progress);
		};
		cbs.on_done = [](void* ctx, const char* name, int available, int total, int /*targetExists*/, int /*isComplete*/) {
			auto* self = static_cast<Repairer*>(ctx);
			self->m_owner->SignalDone(name ? name : "", available, total);
		};
		cbs.on_quick_verify = [](void* ctx, const char* name, uint32_t* outCrc, const NzbgetRsParSegment** outSegs, size_t* outSegLen) -> int {
			auto* self = static_cast<Repairer*>(ctx);
			uint32 crc = 0;
			ParChecker::SegmentList segments;
			ParChecker::EFileStatus st = self->m_owner->FindFileCrc(name ? name : "", &crc, &segments);
			if (outCrc)
			{
				*outCrc = crc;
			}
			self->m_lastSegments.clear();
			for (ParChecker::Segment& seg : segments)
			{
				self->m_lastSegments.push_back({
					seg.GetSuccess() ? 1 : 0,
					seg.GetOffset(),
					seg.GetSize(),
					seg.GetCrc()
				});
			}
			if (outSegs)
			{
				*outSegs = self->m_lastSegments.empty() ? nullptr : self->m_lastSegments.data();
			}
			if (outSegLen)
			{
				*outSegLen = self->m_lastSegments.size();
			}
			return (int)st;
		};
		cbs.on_quick_verified_inc = [](void* ctx) {
			auto* self = static_cast<Repairer*>(ctx);
			self->m_owner->m_quickFiles++;
			self->m_owner->SetParFull(false);
		};
		return cbs;
	}

	ParChecker* m_owner;
	NzbgetRsParRepairer* m_handle = nullptr;
	int m_threadsToUse = 1;
	int m_memToUse = 16;
	std::vector<NzbgetRsParSegment> m_lastSegments;

	friend class ParChecker;
};

inline int ParChecker::StreamBuf::overflow(int ch)
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


inline void ParChecker::Cleanup()
{
	std::lock_guard guard{ m_repairerMtx };

	m_repairer.reset();
	m_queuedParFiles.clear();
	m_processedFiles.clear();
	m_dupeSources.clear();
	m_errMsg = "";

	MemoryMonitor::TrimHeap();
}

inline void ParChecker::Execute()
{
	m_verifiedFiles.clear();
	m_status = RunParCheckAll();

	if (m_status == psRepairNotNeeded && m_parQuick && m_forceRepair && !IsStopped())
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

inline ParChecker::EStatus ParChecker::RunParCheckAll()
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
			if (allStatus > status)
			{
				allStatus = status;
			}
		}
	}

	return allStatus;
}

inline ParChecker::EStatus ParChecker::RunParCheck(std::string parFilename)
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

	m_progressLabel = std::string("Verifying ") + m_infoName;
	m_fileProgress = 0;
	m_stageProgress = 0;
	UpdateProgress();

	int res = PreProcessPar();
	if (IsStopped() || res != NZBGET_RS_PAR_RESULT_SUCCESS)
	{
		Cleanup();
		return psFailed;
	}

	std::string creator = GetPacketCreator();
	info("Recovery files created by: %s", creator.empty() ? "unknown" : creator.c_str());

	m_stage = ptVerifyingSources;
	res = GetRepairer()->Process(false);

	if (!m_parQuick)
	{
		CheckEmptyFiles();
	}

	bool addedSplittedFragments = false;
	if (m_hasDamagedFiles && !IsStopped() && res == NZBGET_RS_PAR_RESULT_REPAIR_NOT_POSSIBLE)
	{
		addedSplittedFragments = AddSplittedFragments();
		if (addedSplittedFragments)
		{
			res = GetRepairer()->Process(false);
		}
	}

	if (m_hasDamagedFiles && !IsStopped() && GetRepairer()->MissingFileCount() > 0 &&
		!(addedSplittedFragments && res == NZBGET_RS_PAR_RESULT_REPAIR_POSSIBLE) &&
		(g_Options->GetParScan() == Options::psExtended || g_Options->GetParScan() == Options::psDupe))
	{
		if (AddMissingFiles())
		{
			res = GetRepairer()->Process(false);
		}
	}

	if (m_hasDamagedFiles && !IsStopped() && res == NZBGET_RS_PAR_RESULT_REPAIR_NOT_POSSIBLE)
	{
		res = ProcessMorePars();
	}

	if (m_hasDamagedFiles && !IsStopped() && res == NZBGET_RS_PAR_RESULT_REPAIR_NOT_POSSIBLE &&
		g_Options->GetParScan() == Options::psDupe)
	{
		if (AddDupeFiles())
		{
			res = GetRepairer()->Process(false);
			if (res == NZBGET_RS_PAR_RESULT_REPAIR_NOT_POSSIBLE && !IsStopped())
			{
				res = ProcessMorePars();
			}
		}
	}

	// a twin's par2 set (other block size, other parity level, same files) may hold
	// the recovery blocks its own lacks
	if (m_hasDamagedFiles && !IsStopped() && res == NZBGET_RS_PAR_RESULT_REPAIR_NOT_POSSIBLE)
	{
		res = ProcessTwinSet();
	}

	if (IsStopped())
	{
		Cleanup();
		return psFailed;
	}

	status = psFailed;

	int postVerifyAction = 0;
	status = (EStatus)nzbget_rs_parchecker_eval_post_verify_action(
		res, NZBGET_RS_PAR_RESULT_SUCCESS, NZBGET_RS_PAR_RESULT_REPAIR_POSSIBLE,
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
		m_progressLabel = std::string("Repairing ") + m_infoName;
		m_fileProgress = 0;
		m_stageProgress = 0;
		m_processedCount = 0;
		m_stage = ptRepairing;
		m_filesToRepair = static_cast<int>(GetRepairer()->DamagedFileCount() + GetRepairer()->MissingFileCount());
		UpdateProgress();

		res = GetRepairer()->Process(true);
		if (res == NZBGET_RS_PAR_RESULT_SUCCESS)
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
	status = (EStatus)nzbget_rs_parchecker_eval_cancel_or_fail_outcome(
		IsStopped() ? 1 : 0, (int)m_stage, (int)ptRepairing, (int)status,
		(int)psFailed, (int)psRepairPossible, m_errMsg.empty() ? 1 : 0, res,
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
			m_errMsg = nzbget_rs_parchecker_cmdline_err_str(res);
		}
		PrintMessage(Message::mkError, "Repair failed for %s: %s. Recovery files created by: %s",
			m_infoName.c_str(), m_errMsg.c_str(), creator.empty() ? "unknown" : creator.c_str());
	}

	// A successful set does not cover unrelated files, and ignored damage
	// can also produce psRepairNotNeeded. Keep only complete final targets
	// actually established by par-rs, before its source metadata is freed.
	size_t sfCount = GetRepairer()->SourceFileCount();
	for (size_t i = 0; i < sfCount; ++i)
	{
		std::string targetPath;
		bool isCompleteTarget = false;
		if (!GetRepairer()->SourceFileInfo(i, nullptr, &targetPath, nullptr, &isCompleteTarget, nullptr, nullptr))
		{
			continue;
		}
		// A later set may have attempted to rewrite the same target; its
		// result supersedes any earlier proof, including when repair failed.
		m_verifiedFiles.erase(std::remove_if(m_verifiedFiles.begin(), m_verifiedFiles.end(),
			[&targetPath](const std::string& previous)
			{
				return FileSystem::SameFilename(previous.c_str(), targetPath.c_str());
			}), m_verifiedFiles.end());
		if ((status == psRepaired || status == psRepairNotNeeded) && isCompleteTarget)
		{
			m_verifiedFiles.push_back(std::move(targetPath));
		}
	}

	Cleanup();
	return status;
}

inline int ParChecker::PreProcessPar()
{
	int res = NZBGET_RS_PAR_RESULT_REPAIR_FAILED;
	while (!IsStopped() && res != NZBGET_RS_PAR_RESULT_SUCCESS)
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
			return NZBGET_RS_PAR_RESULT_REPAIR_FAILED;
		}

		if (res == NZBGET_RS_PAR_RESULT_INVALID_ARGS)
		{
			PrintMessage(Message::mkError, "Could not start par-check for %s. Par-file: %s", m_infoName.c_str(), m_parFilename.c_str());
			m_errMsg = "Command line could not be parsed";
			return res;
		}

		if (res != NZBGET_RS_PAR_RESULT_SUCCESS)
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

	if (res != NZBGET_RS_PAR_RESULT_SUCCESS)
	{
		PrintMessage(Message::mkError, "Could not verify %s: par2-file could not be processed", m_infoName.c_str());
		m_errMsg = "par2-file could not be processed";
		return res;
	}

	return res;
}

inline bool ParChecker::LoadMainParBak()
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

		if (!requested && !hasMorePars)
		{
			return false;
		}

		if (!hasMorePars)
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

inline int ParChecker::ProcessMorePars()
{
	int res = NZBGET_RS_PAR_RESULT_REPAIR_NOT_POSSIBLE;

	bool moreFilesLoaded = true;
	while (!IsStopped() && res == NZBGET_RS_PAR_RESULT_REPAIR_NOT_POSSIBLE)
	{
		int missingblockcount = static_cast<int>(GetRepairer()->MissingBlockCount()) -
			static_cast<int>(GetRepairer()->RecoveryBlockCount());
		if (missingblockcount <= 0)
		{
			return NZBGET_RS_PAR_RESULT_REPAIR_POSSIBLE;
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

			if (!requested && !hasMorePars)
			{
				NzbgetRsBuf errBuf = nzbget_rs_parchecker_format_not_enough_blocks_errmsg(
					missingblockcount, blockFound);
				m_errMsg = errBuf.data ? std::string(errBuf.data, errBuf.len) : std::string();
				nzbget_rs_free(errBuf);
				break;
			}

			if (!hasMorePars)
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
			res = GetRepairer()->Process(false);
		}
	}

	return res;
}

inline int ParChecker::ProcessTwinSet()
{
	std::string index = RequestTwinIndex();
	if (index.empty() || IsStopped())
	{
		return NZBGET_RS_PAR_RESULT_REPAIR_NOT_POSSIBLE;
	}

	// a set of its own (its packets carry another set id): verified anew, with
	// the files on disk found by the names in its FileDesc packets
	std::string ownParFilename = m_parFilename;
	std::string ownErrMsg = m_errMsg;
	m_parFilename = index;
	m_errMsg.clear();
	int res = PreProcessPar();
	if (res != NZBGET_RS_PAR_RESULT_SUCCESS || IsStopped())
	{
		m_parFilename = ownParFilename;
		m_errMsg = ownErrMsg;
		return NZBGET_RS_PAR_RESULT_REPAIR_NOT_POSSIBLE;
	}

	res = GetRepairer()->Process(false);
	if (res == NZBGET_RS_PAR_RESULT_REPAIR_NOT_POSSIBLE && !IsStopped())
	{
		int blockNeeded = std::max(1, static_cast<int>(GetRepairer()->MissingBlockCount()) -
			static_cast<int>(GetRepairer()->RecoveryBlockCount()));
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
			res = GetRepairer()->Process(false);
		}
	}

	NzbgetRsBuf twinErrBuf = nzbget_rs_parchecker_format_twin_err_msg(
		res, NZBGET_RS_PAR_RESULT_REPAIR_NOT_POSSIBLE, ownErrMsg.c_str());
	if (twinErrBuf.data)
	{
		m_errMsg.assign(twinErrBuf.data, twinErrBuf.len);
		nzbget_rs_free(twinErrBuf);
	}
	return res;
}

inline bool ParChecker::LoadMorePars()
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

inline void ParChecker::AddParFile(const char * parFilename)
{
	std::lock_guard guard{ m_queuedParFilesMtx };
	m_queuedParFiles.push_back(parFilename);
	m_queuedParFilesChanged = true;
}

inline void ParChecker::QueueChanged()
{
	std::lock_guard guard{ m_queuedParFilesMtx };
	m_queuedParFilesChanged = true;
}

inline bool ParChecker::AddSplittedFragments()
{
	std::vector<std::string> extrafiles;

	size_t sfCount = GetRepairer()->SourceFileCount();
	DirBrowser dir(m_destDir.c_str());
	while (const char* filename = dir.Next())
	{
		if (!IsParredFile(filename) && !IsProcessedFile(filename))
		{
			for (size_t i = 0; i < sfCount; ++i)
			{
				std::string target;
				if (!GetRepairer()->SourceFileInfo(i, &target, nullptr, nullptr, nullptr, nullptr, nullptr))
				{
					continue;
				}
				const char* current = FileSystem::BaseFileName(target.c_str());

				// if file was renamed by par-renamer we also check the original filename
				const char* original = FindFileOrigname(current);

				if (MaybeSplittedFragement(filename, current) ||
					(original && strcasecmp(original, current) &&
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
		fragmentsAdded = GetRepairer()->VerifyExtraFiles(extrafiles, true);
		m_verifyingExtraFiles = false;
	}

	return fragmentsAdded;
}

inline bool ParChecker::MaybeSplittedFragement(const char* filename1, const char* filename2)
{
	return nzbget_rs_parchecker_maybe_splitted_fragment(filename1, filename2) != 0;
}

inline bool ParChecker::AddMissingFiles()
{
	return AddExtraFiles(true, false, m_destDir.c_str());
}

inline bool ParChecker::AddDupeFiles()
{
	// the folder of the par2 file, not the file itself (which is no folder to scan)
	BString<1024> directory;
	const char* lastSep = strrchr(m_parFilename.c_str(), PATH_SEPARATOR);
	size_t dirLen = lastSep ? static_cast<size_t>(lastSep - m_parFilename.c_str()) : m_parFilename.size();
	directory.Set(m_parFilename.c_str(), (int)dirLen);

	bool added = AddExtraFiles(false, false, directory);

	if (GetRepairer()->MissingBlockCount() > 0)
	{
		// scanning directories of duplicates
		RequestDupeSources(&m_dupeSources);

		if (!m_dupeSources.empty())
		{
			uint32_t wasBlocksMissing = GetRepairer()->MissingBlockCount();
			uint32_t totalExtraBlocksFound = 0;
			for (DupeSource& dupeSource : m_dupeSources)
			{
				if (GetRepairer()->MissingBlockCount() > 0 && FileSystem::DirectoryExists(dupeSource.GetDirectory()))
				{
					uint32_t wasBlocksMissing2 = GetRepairer()->MissingBlockCount();
					bool oneAdded = AddExtraFiles(false, true, dupeSource.GetDirectory());
					added |= oneAdded;
					uint32_t blocksMissing2 = GetRepairer()->MissingBlockCount();
					dupeSource.SetUsedBlocks(nzbget_rs_parchecker_calc_dupe_blocks_delta(
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
inline void ParChecker::SortExtraFiles(std::vector<std::string>& extrafiles)
{
	NzbgetRsBuf stemBuf = nzbget_rs_parchecker_extract_base_par_stem(m_parFilename.c_str());
	std::string baseParFilename = stemBuf.data ? std::string(stemBuf.data, stemBuf.len) : std::string();
	nzbget_rs_free(stemBuf);

	std::sort(
		begin(extrafiles),
		end(extrafiles),
		[&baseParFilename](const std::string& a, const std::string& b)
		{
			return nzbget_rs_parchecker_compare_extra_file_stems(
				FileSystem::BaseFileName(a.c_str()),
				FileSystem::BaseFileName(b.c_str()),
				baseParFilename.c_str()) != 0;
		}
	);
}

// (the list by reference: taken by value, every file found was thrown away, and the
// extended and dupe par-scans never added a block)
inline void ParChecker::FindExtraFiles(std::vector<std::string>& extrafiles, const char* directory, bool externalDir)
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
			if (externalDir || (!isParred && !isProcessed))
			{
				extrafiles.push_back(std::move(fullFilenameStr));
			}
		}
	}
}

inline bool ParChecker::AddExtraFiles(bool onlyMissing, bool externalDir, const char* directory)
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

			uint32_t wasFilesMissing = GetRepairer()->MissingFileCount();
			uint32_t wasBlocksMissing = GetRepairer()->MissingBlockCount();

			GetRepairer()->VerifyExtraFiles({ extraFile }, false);

			int fileAdded = 0;
			int blockAdded = 0;
			uint32_t blocksDelta = 0;
			int registerParred = 0;
			int abortReason = 0;
			filesAdded = nzbget_rs_parchecker_eval_extra_file_step(
				wasFilesMissing, GetRepairer()->MissingFileCount(),
				wasBlocksMissing, GetRepairer()->MissingBlockCount(),
				externalDir ? 1 : 0, filesAdded ? 1 : 0, onlyMissing ? 1 : 0,
				&fileAdded, &blockAdded, &blocksDelta, &registerParred, &abortReason) != 0;
			if (registerParred != 0)
			{
				PrintMessage(Message::mkInfo, "Found missing file %s", FileSystem::BaseFileName(extraFile.c_str()));
				RegisterParredFile(FileSystem::BaseFileName(extraFile.c_str()));
			}
			else if (blockAdded != 0)
			{
				PrintMessage(Message::mkInfo, "Found %u missing blocks", blocksDelta);
			}

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

inline bool ParChecker::IsProcessedFile(const char* filename)
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

inline void ParChecker::SignalFilename(std::string str)
{
	std::lock_guard guard{ m_signalMtx };

	if (m_stage == ptRepairing)
	{
		m_stage = ptVerifyingRepaired;
	}

	// don't print progress messages when verifying repaired files in quick verification mode,
	// because repaired files are not verified in this mode
	const char* stageMsg = nzbget_rs_parchecker_stage_message((int)m_stage);
	if (!(m_stage == ptVerifyingRepaired && m_parQuick))
	{
		PrintMessage(Message::mkInfo, "%s %s", stageMsg, str.c_str());
	}

	m_progressLabel = std::string(stageMsg) + " " + str;

	if (m_stage == ptLoadingPars || m_stage == ptVerifyingSources)
	{
		m_processedFiles.push_back(std::move(str));
	}

	m_fileProgress = 0;
	UpdateProgress();
}

inline void ParChecker::SignalProgress(int progress)
{
	std::lock_guard guard{ m_signalMtx };

	m_fileProgress = progress;

	int sourceFilesCount = (m_stage == ptRepairing || m_stage == ptVerifyingRepaired)
		? 0
		: static_cast<int>(GetRepairer()->SourceFileCount());
	m_stageProgress = nzbget_rs_parchecker_calc_stage_progress(
		(int)m_stage, m_fileProgress, m_processedCount, m_filesToRepair,
		sourceFilesCount, m_extraFiles, m_quickFiles);
	debug("Current-progress: %i, Total-progress: %i", m_fileProgress, m_stageProgress);

	UpdateProgress();
}

inline void ParChecker::SignalDone(std::string fileName, int available, int total)
{
	std::lock_guard guard{ m_signalMtx };

	++m_processedCount;

	bool ignore = (m_stage == ptVerifyingSources && available < total && !m_verifyingExtraFiles)
		? Util::MatchFileExt(fileName.c_str(), g_Options->GetParIgnoreExt(), ",;")
		: false;
	int newHasDamaged = m_hasDamagedFiles ? 1 : 0;
	int badBlocks = 0;
	if (nzbget_rs_parchecker_eval_signal_done(
		(int)m_stage, (int)ptVerifyingSources, available, total,
		m_verifyingExtraFiles ? 1 : 0, ignore ? 1 : 0,
		m_hasDamagedFiles ? 1 : 0, &newHasDamaged, &badBlocks) != 0)
	{
		m_hasDamagedFiles = newHasDamaged != 0;
		bool fileExists = true;
		size_t sfCount = GetRepairer()->SourceFileCount();
		for (size_t i = 0; i < sfCount; ++i)
		{
			std::string targetFileName;
			bool exists = false;
			if (!GetRepairer()->SourceFileInfo(i, &targetFileName, nullptr, &exists, nullptr, nullptr, nullptr))
			{
				continue;
			}
			if (!exists &&
				!strcmp(fileName.c_str(), FileSystem::BaseFileName(targetFileName.c_str())))
			{
				fileExists = false;
				break;
			}
		}

		const char* ignoreSuffix = ignore ? " (ignored)" : "";
		if (fileExists)
		{
			PrintMessage(Message::mkWarning, "File %s has %i bad block(s) of total %i block(s)%s",
				fileName.c_str(), badBlocks, total, ignoreSuffix);
		}
		else
		{
			PrintMessage(Message::mkWarning, "File %s with %i block(s) is missing%s",
				fileName.c_str(), total, ignoreSuffix);
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
inline void ParChecker::CheckEmptyFiles()
{
	size_t sfCount = GetRepairer()->SourceFileCount();
	for (size_t i = 0; i < sfCount; ++i)
	{
		std::string filenameObj;
		uint32_t total = 0;
		if (!GetRepairer()->SourceFileInfo(i, &filenameObj, nullptr, nullptr, nullptr, &total, nullptr))
		{
			continue;
		}
		if (!filenameObj.empty() && !IsProcessedFile(filenameObj.c_str()))
		{
			bool ignore = Util::MatchFileExt(filenameObj.c_str(), g_Options->GetParIgnoreExt(), ",;");
			m_hasDamagedFiles |= !ignore;

			PrintMessage(Message::mkWarning, "File %s has %u bad block(s) of total %u block(s)%s",
				filenameObj.c_str(), total, total, ignore ? " (ignored)" : "");
		}
	}
}

inline void ParChecker::Cancel()
{
	{
		std::lock_guard guard{ m_repairerMtx };
		if (m_repairer)
		{
			m_repairer->GetRepairer()->Cancel();
		}
	}
	QueueChanged();
}

inline void ParChecker::SaveSourceList()
{
	// Leftovers are tracked directly by RustParRepairer during repair().
}

inline void ParChecker::DeleteLeftovers()
{
	size_t count = GetRepairer()->LeftoverCount();
	for (size_t i = 0; i < count; ++i)
	{
		std::string fileName = GetRepairer()->LeftoverPath(i);
		if (!fileName.empty())
		{
			PrintMessage(Message::mkInfo, "Deleting file %s", FileSystem::BaseFileName(fileName.c_str()));
			FileSystem::DeleteFile(fileName.c_str());
		}
	}
}

/*
 * Compute CRC of bytes range of file using CRCs of segments and reading some data directly
 * from file if necessary
 */
inline bool ParChecker::SmartCalcFileRangeCrc(DiskFile& file, int64 start, int64 end, SegmentList& segments,
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
inline bool ParChecker::DumbCalcFileRangeCrc(DiskFile& file, int64 start, int64 end, uint32& downloadCrcOut)
{
	if (!file.Seek(start))
	{
		return false;
	}

	CharBuffer buffer(1024 * 64);
	Crc32 downloadCrc;

	int cnt = buffer.Size();
	while (cnt == buffer.Size() && start < end)
	{
		int needBytes = end - start + 1 > buffer.Size() ? buffer.Size() : (int)(end - start + 1);
		cnt = (int)file.Read(buffer, needBytes);
		downloadCrc.Append((uchar*)(char*)buffer, cnt);
		start += cnt;
	}

	downloadCrcOut = downloadCrc.Finish();
	return true;
}

inline std::string ParChecker::GetPacketCreator()
{
	if (!m_repairer || !GetRepairer())
	{
		return "";
	}
	return GetRepairer()->Creator();
}

inline int64 ParChecker::GetBlockSize()
{
	if (!m_repairer || !GetRepairer())
	{
		return 0;
	}
	return GetRepairer()->BlockSize();
}

#endif

#endif
