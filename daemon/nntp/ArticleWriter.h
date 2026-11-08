/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2014-2019 Andrey Prygunkov <hugbug@users.sourceforge.net>
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


#ifndef ARTICLEWRITER_H
#define ARTICLEWRITER_H

#include <atomic>
#include <optional>
#include <string>
#include "NString.h"
#include "DownloadInfo.h"
#include "Options.h"
#include "FileSystem.h"

class CachedSegmentData final : public SegmentData
{
public:
	CachedSegmentData() {}
	CachedSegmentData(char* data, int size) : m_data(data), m_size(size) {}
	CachedSegmentData(const CachedSegmentData&) = delete;
	CachedSegmentData(CachedSegmentData&& other) :
		m_data(other.m_data), m_size(other.m_size) { other.m_data = nullptr; other.m_size = 0; }
	CachedSegmentData& operator=(CachedSegmentData&& other);
	~CachedSegmentData() override;
	char* GetData() override { return m_data; }

private:
	char* m_data = nullptr;
	int m_size = 0;

	friend class ArticleCache;
};

class ArticleWriter
{
public:
	// more bytes decoded than the article's range holds (they weren't written)
	bool GetRangeExceeded() { return m_articlePtr > m_articleSize; }
	struct OutputPaths 
	{
	    std::string finalPath;
	    std::string tempPath;
	};
	void SetInfoName(const char* infoName) { m_infoName = infoName ? infoName : ""; }
	void SetInfoName(std::string infoName) { m_infoName = std::move(infoName); }
	void SetFileInfo(FileInfo* fileInfo) { m_fileInfo = fileInfo; }
	void SetArticleInfo(ArticleInfo* articleInfo) { m_articleInfo = articleInfo; }
	void Prepare();
	bool Start(Decoder::EFormat format, const char* filename, int64 fileSize, int64 articleOffset, int articleSize);
	bool Write(char* buffer, int len);
	void Finish(bool success);
	bool GetDuplicate() { return m_duplicate; }
	bool HasCachedBuffer() { return m_articleData.GetData() != nullptr; }
	void LogStartMessage(std::string_view infoFilename, bool directWrite, bool cached);
	std::optional<OutputPaths> SetupOutputFile(DiskFile &outfile,
											std::string_view destDir,
											std::string_view filename,
											std::string_view infoFilename,
											bool directWrite,
											bool cached);
	uint32 ProcessArticles(DiskFile& outfile,
						std::string_view infoFilename,
						std::string_view finalOutputPath,
						bool directWrite,
						bool cached);
	bool CommitDiskFile(DiskFile& outfile,
						std::string_view tempDestPath,
						std::string_view finalOutputPath,
						bool directWrite);
	bool CleanupOldData(bool directWrite,
						std::string_view nzbDestDir,
						std::string_view finalOutputPath);
	void ReportCompletionStatus(std::string_view infoFilename);
	void HandlePostProcessing(uint32 crc, 
							std::string_view currentPath, 
							std::string_view originalFilename, 
							std::string_view nzbDestDir);
	bool CompleteFileParts();
	static bool MoveCompletedFiles(NzbInfo* nzbInfo, const char* oldDestDir);
	void FlushCache();

private:
	bool IsDupeFallbackArticle() const;
	bool GetSkipDiskWrite();

	FileInfo* m_fileInfo;
	ArticleInfo* m_articleInfo;
	DiskFile m_outFile;
	std::string m_tempFilename;
	std::string m_outputFilename;
	std::string m_resultFilename;
	std::string m_infoName;
	Decoder::EFormat m_format = Decoder::efUnknown;
	CachedSegmentData m_articleData;
	int64 m_articleOffset;
	int m_articleSize;
	int m_articlePtr;
	bool m_duplicate = false;
	bool m_commitError = false;

	bool CreateOutputFile(int64 size);
	void BuildOutputFilename();
	void SetWriteBuffer(DiskFile& outFile, int recSize);
};

class ArticleCache final : public Thread
{
public:
	class FlushGuard
	{
	public:
		FlushGuard(FlushGuard&& other) = default;
		~FlushGuard();
	private:
		Guard m_guard;
		FlushGuard(Mutex& mutex);
		friend class ArticleCache;
	};

	void Run() override;
	void Stop() override;
	CachedSegmentData Alloc(int size);
	bool Realloc(CachedSegmentData* segment, int newSize);
	void Free(CachedSegmentData* segment);
	FlushGuard GuardFlush() { return FlushGuard(m_flushMutex); }
	Guard GuardContent() { return Guard(m_contentMutex); }
	bool GetFlushing() { return m_flushing; }
	size_t GetAllocated() { return m_allocated; }
	bool FileBusy(FileInfo* fileInfo) { return fileInfo == m_fileInfo; }
	void WakeupFlush()
	{
		std::lock_guard<std::mutex> guard(m_allocMutex);
		m_allocCond.notify_all();
	}

private:
	std::atomic<size_t> m_allocated{0};
	FileInfo* m_fileInfo = nullptr;
	std::mutex m_allocMutex;
	std::condition_variable m_allocCond;
	Mutex m_flushMutex;
	Mutex m_contentMutex;
	bool m_flushing = false;

	bool CheckFlush(bool flushEverything);
};

extern ArticleCache* g_ArticleCache;

#include <sstream>
#include <iomanip>
#include "DiskState.h"
#include "Options.h"
#include "Log.h"
#include "Util.h"
#include "FileSystem.h"

inline CachedSegmentData::~CachedSegmentData()
{
	if (g_ArticleCache)
	{
		g_ArticleCache->Free(this);
	}
}

inline CachedSegmentData& CachedSegmentData::operator=(CachedSegmentData&& other)
{
	if (g_ArticleCache)
	{
		g_ArticleCache->Free(this);
	}
	m_data = other.m_data;
	m_size = other.m_size;
	other.m_data = nullptr;
	other.m_size = 0;
	return *this;
}


inline void ArticleWriter::SetWriteBuffer(DiskFile& outFile, int recSize)
{
	int bufSize = nzbget_rs_qhp_calc_write_buffer_size(g_Options->GetWriteBuffer(), recSize);
	if (bufSize > 0)
	{
		outFile.SetWriteBuffer(bufSize);
	}
}

inline void ArticleWriter::Prepare()
{
	BuildOutputFilename();
	m_resultFilename = m_articleInfo->GetResultFilename()
		? m_articleInfo->GetResultFilename()
		: "";
}

inline bool ArticleWriter::IsDupeFallbackArticle() const
{
	return nzbget_rs_dqh_article_writer_is_dupe_fallback_article(
		m_articleInfo ? 1 : 0,
		m_articleInfo ? m_articleInfo->GetDupeFallbackRound() : 0) != 0;
}

inline bool ArticleWriter::Start(Decoder::EFormat format, const char* filename, int64 fileSize,
	int64 articleOffset, int articleSize)
{
	m_outFile.Close();
	m_format = format;
	m_articleOffset = articleOffset;
	m_articleSize = articleSize > 0 ? articleSize : m_articleInfo->GetSize();
	m_articlePtr = 0;

	// prepare file for writing
	if (m_format == Decoder::efYenc)
	{
		if (g_Options->GetDupeCheck() &&
			m_fileInfo->GetNzbInfo()->GetDupeMode() != dmForce &&
			!m_fileInfo->GetNzbInfo()->GetManyDupeFiles())
		{
			bool outputInitialized;
			{
				Guard guard = m_fileInfo->GuardOutputFile();
				outputInitialized = m_fileInfo->GetOutputInitialized();
				if (!g_Options->GetDirectWrite() && !m_fileInfo->GetForceDirectWrite())
				{
					m_fileInfo->SetOutputInitialized(true);
				}
			}

			BString<1024> destFilePath;
			if (filename)
			{
				NzbgetRsBuf dfpBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
					m_fileInfo->GetNzbInfo()->GetDestDir(), PATH_SEPARATOR, filename);
				destFilePath.Set(dfpBuf.data ? dfpBuf.data : "", (int)dfpBuf.len);
				nzbget_rs_free(dfpBuf);
			}
			bool isOwnOutputFile = false;
			if (g_Options->GetDirectWrite() || m_fileInfo->GetForceDirectWrite())
			{
				Guard guard = m_fileInfo->GuardOutputFile();
				isOwnOutputFile = m_fileInfo->GetForceDirectWrite() ||
					(!m_fileInfo->GetOutputFilename().empty() &&
					 FileSystem::SameFilename(m_fileInfo->GetOutputFilename().c_str(), destFilePath));
			}

			if (!outputInitialized && !isOwnOutputFile && filename &&
				FileSystem::FileExists(destFilePath))
			{
				m_duplicate = true;
				return false;
			}
		}

		if (g_Options->GetDirectWrite() && !IsDupeFallbackArticle())
		{
			Guard guard = m_fileInfo->GuardOutputFile();
			if (!m_fileInfo->GetOutputInitialized() ||
				(!GetSkipDiskWrite() && !FileSystem::FileExists(m_outputFilename.c_str())))
			{
				if (!GetSkipDiskWrite() && !CreateOutputFile(fileSize))
				{
					return false;
				}
				m_fileInfo->SetOutputInitialized(true);
			}
		}
	}

	// allocate cache buffer; yEnc only: a cached article is written at its offset,
	bool isCritical = g_MemoryMonitor &&
		g_MemoryMonitor->GetTier() == NZBGET_RS_PRESSURE_CRITICAL;

	// allocate cache buffer; yEnc only: a cached article is written at its offset,
	// and only yEnc tells it (uuencoded parts all have 0 and overwrote each other,
	// uncached they're appended in order)
	if (g_Options->GetArticleCache() > 0 && !g_Options->GetRawArticle() &&
		m_format == Decoder::efYenc && !IsDupeFallbackArticle() && !isCritical)
	{
		m_articleData = g_ArticleCache->Alloc(m_articleSize);

		while (!m_articleData.GetData() && g_ArticleCache->GetFlushing() &&
			!(g_MemoryMonitor && g_MemoryMonitor->GetTier() == NZBGET_RS_PRESSURE_CRITICAL))
		{
			Util::Sleep(5);
			m_articleData = g_ArticleCache->Alloc(m_articleSize);
		}

		if (!m_articleData.GetData())
		{
			detail("Article cache is full, using disk for %s", m_infoName.c_str());
		}
	}

	if (!m_articleData.GetData())
	{
		if (GetSkipDiskWrite())
		{
			return true;
		}

		bool directWrite = (g_Options->GetDirectWrite() || m_fileInfo->GetForceDirectWrite()) &&
			m_format == Decoder::efYenc && !IsDupeFallbackArticle();
		const char* outFilename = directWrite ? m_outputFilename.c_str() : m_tempFilename.c_str();
		if (!m_outFile.Open(outFilename, directWrite ? DiskFile::omReadWrite : DiskFile::omWrite))
		{
			m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError,
				"Could not %s file %s: %s", nzbget_rs_dqh_article_writer_open_verb(directWrite ? 1 : 0), outFilename,
				*FileSystem::GetLastErrorMessage());
			return false;
		}
		SetWriteBuffer(m_outFile, m_articleInfo->GetSize());

		if (directWrite)
		{
			m_outFile.Seek(m_articleOffset);
		}
	}

	return true;
}

inline bool ArticleWriter::GetSkipDiskWrite()
{
	return (m_fileInfo && m_fileInfo->GetNzbInfo() && m_fileInfo->GetNzbInfo()->GetSkipDiskWrite()) ||
		g_Options->GetSkipWrite();
}

inline bool ArticleWriter::Write(char* buffer, int len)
{
	if (nzbget_rs_awc_article_writer_advance_and_check_range(
		g_Options->GetRawArticle() ? 1 : 0,
		&m_articlePtr,
		len,
		m_articleSize) != 0)
	{
		return true;
	}

	if (!g_Options->GetRawArticle() && m_articleData.GetData())
	{
		memcpy(m_articleData.GetData() + m_articlePtr - len, buffer, len);
		return true;
	}

	if (GetSkipDiskWrite())
	{
		return true;
	}

	return m_outFile.Write(buffer, len) > 0;
}

inline void ArticleWriter::Finish(bool success)
{
	m_outFile.Close();

	if (!success)
	{
		if (!GetSkipDiskWrite())
		{
			FileSystem::DeleteFile(m_tempFilename.c_str());
			FileSystem::DeleteFile(m_resultFilename.c_str());
		}
		return;
	}

	bool directWrite = (g_Options->GetDirectWrite() || m_fileInfo->GetForceDirectWrite()) &&
		m_format == Decoder::efYenc && !IsDupeFallbackArticle();

	if (!g_Options->GetRawArticle())
	{
		if (!directWrite && !m_articleData.GetData() && !GetSkipDiskWrite())
		{
			if (!FileSystem::MoveFile(m_tempFilename.c_str(), m_resultFilename.c_str()))
			{
				m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError,
					"Could not rename file %s to %s: %s", m_tempFilename.c_str(), m_resultFilename.c_str(),
					*FileSystem::GetLastErrorMessage());
			}
			FileSystem::DeleteFile(m_tempFilename.c_str());
		}

		if (m_articleData.GetData())
		{
			if (m_articleSize != m_articlePtr)
			{
				g_ArticleCache->Realloc(&m_articleData, m_articlePtr);
			}
			Guard contentGuard = g_ArticleCache->GuardContent();
			m_articleInfo->AttachSegment(std::make_unique<CachedSegmentData>(std::move(m_articleData)), m_articleOffset, m_articlePtr);
			m_fileInfo->SetCachedArticles(m_fileInfo->GetCachedArticles() + 1);
		}
		else
		{
			m_articleInfo->SetSegmentOffset(m_articleOffset);
			m_articleInfo->SetSegmentSize(m_articlePtr);
		}
	}
	else
	{
		// rawmode
		if (!GetSkipDiskWrite())
		{
			if (!FileSystem::MoveFile(m_tempFilename.c_str(), m_resultFilename.c_str()))
			{
				m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError,
					"Could not move file %s to %s: %s", m_tempFilename.c_str(), m_resultFilename.c_str(),
					*FileSystem::GetLastErrorMessage());
			}
		}
	}
}

/* creates output file and subdirectores */
inline bool ArticleWriter::CreateOutputFile(int64 size)
{
	bool outExists = FileSystem::FileExists(m_outputFilename.c_str());
	int existingStatus = nzbget_rs_dqh_article_writer_check_existing_output_file(
		outExists ? 1 : 0,
		outExists ? FileSystem::FileSize(m_outputFilename.c_str()) : 0,
		size);
	if (existingStatus == 1)
	{
		// keep existing old file from previous program session
		return true;
	}
	if (existingStatus == 2)
	{
		// delete existing old file from previous program session
		FileSystem::DeleteFile(m_outputFilename.c_str());
	}

	// ensure the directory exist
	BString<1024> destDir;
	destDir.Set(m_outputFilename.c_str(), (int)(FileSystem::BaseFileName(m_outputFilename.c_str()) - m_outputFilename.c_str()));
	CString errmsg;

	if (!FileSystem::ForceDirectories(destDir, errmsg))
	{
		m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError,
			"Could not create directory %s: %s", *destDir, *errmsg);
		return false;
	}

	if (!FileSystem::AllocateFile(m_outputFilename.c_str(), size, g_Options->GetArticleCache() == 0, errmsg))
	{
		m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError,
			"Could not create file %s: %s", m_outputFilename.c_str(), *errmsg);
		return false;
	}

	return true;
}

inline void ArticleWriter::BuildOutputFilename()
{
	if (!m_fileInfo || !m_articleInfo)
		return;

	const char* tmpDir = g_Options->GetTempDir();
	const char* destDir = m_fileInfo->GetNzbInfo()->GetDestDir();
	const bool directWrite = g_Options->GetDirectWrite();
	const bool forceDirectWrite = m_fileInfo->GetForceDirectWrite();
	std::optional<Guard> guard;
	if (directWrite || forceDirectWrite)
	{
		guard.emplace(m_fileInfo->GuardOutputFile());
	}
	const std::string& existingOutputFilename = m_fileInfo->GetOutputFilename();

	auto fileExistsCb = [](void*, const char* path) -> int {
		return FileSystem::FileExists(path) ? 1 : 0;
	};

	NzbgetRsBuf resultBuf{};
	NzbgetRsBuf tempBuf{};
	NzbgetRsBuf outputBuf{};
	int hasOutputFilename = 0;
	int updateFileInfoOutput = 0;
	if (!nzbget_rs_awc_article_writer_build_output_filenames(
		tmpDir,
		destDir,
		PATH_SEPARATOR,
		m_fileInfo->GetId(),
		m_articleInfo->GetPartNumber(),
		directWrite ? 1 : 0,
		forceDirectWrite ? 1 : 0,
		existingOutputFilename.c_str(),
		m_fileInfo->GetOutputInitialized() ? 1 : 0,
		m_fileInfo->GetFilename(),
		nullptr,
		fileExistsCb,
		&resultBuf,
		&tempBuf,
		&outputBuf,
		&hasOutputFilename,
		&updateFileInfoOutput))
	{
		return;
	}

	std::string resultFilename(resultBuf.data ? resultBuf.data : "", resultBuf.len);
	nzbget_rs_free(resultBuf);
	m_articleInfo->SetResultFilename(resultFilename.c_str());

	m_tempFilename.assign(tempBuf.data ? tempBuf.data : "", tempBuf.len);
	nzbget_rs_free(tempBuf);

	if (hasOutputFilename)
	{
		m_outputFilename.assign(outputBuf.data ? outputBuf.data : "", outputBuf.len);
		if (updateFileInfoOutput)
		{
			m_fileInfo->SetOutputFilename(m_outputFilename);
		}
	}
	nzbget_rs_free(outputBuf);
}

inline bool ArticleWriter::CompleteFileParts()
{
	debug("Completing file parts");
	m_commitError = false;
	debug("ArticleFilename: %s", m_fileInfo->GetFilename());

	// 1. Gather context & configuration
	std::string nzbDestDir;
	std::string nzbName;
	std::string filename;

	{
		GuardedDownloadQueue guard = DownloadQueue::Guard();
		nzbName = m_fileInfo->GetNzbInfo()->GetName();
		nzbDestDir = m_fileInfo->GetNzbInfo()->GetDestDir();
		filename = m_fileInfo->GetFilename();
	}

	if (m_outputFilename.empty() && !m_fileInfo->GetOutputFilename().empty())
	{
		m_outputFilename = m_fileInfo->GetOutputFilename();
	}
	bool directWriteOpt = g_Options->GetDirectWrite();
	bool forceDirectWrite = m_fileInfo->GetForceDirectWrite();
	bool curOutputInit = m_fileInfo->GetOutputInitialized();
	bool hasOwnOut = false;
	bool hasForceDest = false;
	if ((directWriteOpt || forceDirectWrite) && !curOutputInit)
	{
		BString<1024> destPath;
		{
			NzbgetRsBuf dpBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
				nzbDestDir.c_str(), PATH_SEPARATOR, filename.c_str());
			destPath.Set(dpBuf.data ? dpBuf.data : "", (int)dpBuf.len);
			nzbget_rs_free(dpBuf);
		}
		hasOwnOut = !m_outputFilename.empty() && FileSystem::FileExists(m_outputFilename.c_str());
		hasForceDest = !hasOwnOut && forceDirectWrite && FileSystem::FileExists(destPath);
	}
	int newOutputInit = curOutputInit ? 1 : 0;
	bool directWrite = nzbget_rs_awc_article_writer_resolve_direct_write_ready(
		directWriteOpt ? 1 : 0,
		forceDirectWrite ? 1 : 0,
		curOutputInit ? 1 : 0,
		hasOwnOut ? 1 : 0,
		hasForceDest ? 1 : 0,
		&newOutputInit) != 0;
	if (newOutputInit && !curOutputInit)
	{
		m_fileInfo->SetOutputInitialized(true);
	}
	bool cached = m_fileInfo->GetCachedArticles() > 0;

	std::string fullInfoPath;
	{
		NzbgetRsBuf infoPathBuf = nzbget_rs_dqh_article_writer_format_info_path(
			nzbName.c_str(), PATH_SEPARATOR, filename.c_str());
		if (infoPathBuf.data)
		{
			fullInfoPath.assign(infoPathBuf.data, infoPathBuf.len);
		}
		nzbget_rs_free(infoPathBuf);
	}
	LogStartMessage(fullInfoPath, directWrite, cached);

	// 2. Return if no successful articles
	if (m_fileInfo->GetSuccessArticles() == 0)
	{
		detail("Skipping file parts completion for %s: no successful articles", fullInfoPath.c_str());
		if (!GetSkipDiskWrite())
		{
			CleanupOldData(directWrite, nzbDestDir, "");
		}
		ReportCompletionStatus(fullInfoPath);
		return true;
	}

	// 3. Skip all filesystem work when SkipWrite is active
	if (GetSkipDiskWrite())
	{
		ReportCompletionStatus(fullInfoPath);
		// Update in-memory FileInfo state only
		{
			GuardedDownloadQueue guard = DownloadQueue::Guard();
			m_fileInfo->SetCrc(0); // CRC not computed in SkipWrite mode
			m_fileInfo->SetOutputFilename("");
		}
		return true;
	}

	// 4. Prepare directory
	CString errmsg;
	if (!FileSystem::ForceDirectories(nzbDestDir.c_str(), errmsg))
	{
		m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError, "Could not create directory %s: %s", nzbDestDir.c_str(), *errmsg);
		return false;
	}

	// 5. Setup output file
	DiskFile outfile;
	auto pathsOpt = SetupOutputFile(outfile, nzbDestDir, filename, fullInfoPath, directWrite, cached);
	if (!pathsOpt)
	{
		return false;
	}

	const auto& [finalOutputPath, tempDestPath] = *pathsOpt;

	// 6. Process data (Write/Join/Move Articles)
	uint32 crc = ProcessArticles(outfile, fullInfoPath, finalOutputPath, directWrite, cached);
	if (m_commitError)
	{
		outfile.Close();
		m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError,
			"Could not commit all article data for %s", fullInfoPath.c_str());
		return false;
	}

	// 7. Finalize file (Close & Move Temp to Final)
	if (!CommitDiskFile(outfile, tempDestPath, finalOutputPath, directWrite))
	{
		return false;
	}

	// 8. Cleanup (Delete parts or old dirs)
	if (!CleanupOldData(directWrite, nzbDestDir, finalOutputPath))
	{
		return false;
	}

	// 9. Report results
	ReportCompletionStatus(fullInfoPath);

	// 10. Post Processing (Renames, Hardlinks, Moves)
	HandlePostProcessing(crc, finalOutputPath, filename, nzbDestDir);
	return true;
}

inline void ArticleWriter::LogStartMessage(std::string_view infoFilename, bool directWrite, bool cached)
{
	if (g_Options->GetRawArticle())
	{
		detail("Moving articles for %s", infoFilename.data());
	}
	else if (directWrite && cached)
	{
		detail("Writing articles for %s", infoFilename.data());
	}
	else if (directWrite)
	{
		detail("Checking articles for %s", infoFilename.data());
	}
	else
	{
		detail("Joining articles for %s", infoFilename.data());
	}
}

inline std::optional<ArticleWriter::OutputPaths>
ArticleWriter::SetupOutputFile(DiskFile &outfile,
							std::string_view destDir,
							std::string_view filename,
							std::string_view infoFilename,
							bool directWrite,
							bool cached)
{
	struct SetupCtx {
		DiskFile* outfile;
		CString forceDirsErr;
	} ctx{&outfile, {}};

	auto sameFilenameCb = [](void*, const char* a, const char* b) -> int {
		return FileSystem::SameFilename(a, b) ? 1 : 0;
	};
	auto makeUniqueCb = [](void*, const char* dir, const char* fname, char* outBuf, size_t outCap) -> size_t {
		CString uniq = FileSystem::MakeUniqueFilename(dir, fname);
		size_t len = uniq.Length();
		if (outBuf && outCap > 0)
		{
			size_t copyLen = len < outCap ? len : outCap - 1;
			memcpy(outBuf, *uniq, copyLen);
			outBuf[copyLen] = '\0';
		}
		return len;
	};
	auto fileExistsCb = [](void*, const char* path) -> int {
		return FileSystem::FileExists(path) ? 1 : 0;
	};
	auto forceDirsCb = [](void* rawCtx, const char* path) -> int {
		auto* c = static_cast<SetupCtx*>(rawCtx);
		return FileSystem::ForceDirectories(path, c->forceDirsErr) ? 1 : 0;
	};
	auto deleteFileCb = [](void*, const char* path) -> int {
		return FileSystem::DeleteFile(path) ? 1 : 0;
	};
	auto openFileCb = [](void* rawCtx, const char* path, int readWrite) -> int {
		auto* c = static_cast<SetupCtx*>(rawCtx);
		return c->outfile->Open(path, readWrite ? DiskFile::omReadWrite : DiskFile::omWrite) ? 1 : 0;
	};
	auto createDirCb = [](void*, const char* path) -> int {
		return FileSystem::CreateDirectory(path) ? 1 : 0;
	};

	NzbgetRsBuf finalBuf{};
	NzbgetRsBuf tempBuf{};
	NzbgetRsBuf updatedOutBuf{};
	NzbgetRsBuf parentFailedBuf{};
	NzbgetRsBuf errPathBuf{};
	int hasParentFailed = 0;
	int errKind = 0;
	int ok = nzbget_rs_awc_article_writer_execute_setup_output_file(
		destDir.data(),
		destDir.size(),
		filename.data(),
		filename.size(),
		PATH_SEPARATOR,
		g_Options->GetRawArticle() ? 1 : 0,
		directWrite ? 1 : 0,
		m_fileInfo->GetForceDirectWrite() ? 1 : 0,
		m_outputFilename.data(),
		m_outputFilename.size(),
		&ctx,
		sameFilenameCb,
		makeUniqueCb,
		fileExistsCb,
		forceDirsCb,
		deleteFileCb,
		openFileCb,
		createDirCb,
		&finalBuf,
		&tempBuf,
		&updatedOutBuf,
		&parentFailedBuf,
		&hasParentFailed,
		&errKind,
		&errPathBuf);

	OutputPaths paths;
	paths.finalPath.assign(finalBuf.data ? finalBuf.data : "", finalBuf.len);
	nzbget_rs_free(finalBuf);
	paths.tempPath.assign(tempBuf.data ? tempBuf.data : "", tempBuf.len);
	nzbget_rs_free(tempBuf);
	m_outputFilename.assign(updatedOutBuf.data ? updatedOutBuf.data : "", updatedOutBuf.len);
	nzbget_rs_free(updatedOutBuf);
	std::string parentFailed(parentFailedBuf.data ? parentFailedBuf.data : "", parentFailedBuf.len);
	nzbget_rs_free(parentFailedBuf);
	std::string errPath(errPathBuf.data ? errPathBuf.data : "", errPathBuf.len);
	nzbget_rs_free(errPathBuf);

	if (hasParentFailed)
	{
		m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError, "Could not create directory %s: %s",
			parentFailed.c_str(), *ctx.forceDirsErr);
	}

	if (!ok)
	{
		if (errKind == 1)
		{
			m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError, "Could not create file %s: %s",
				errPath.c_str(), *FileSystem::GetLastErrorMessage());
		}
		else if (errKind == 2)
		{
			m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError, "Could not open file %s: %s",
				errPath.c_str(), *FileSystem::GetLastErrorMessage());
		}
		else if (errKind == 3)
		{
			m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError, "Could not create directory %s: %s",
				errPath.c_str(), *FileSystem::GetLastErrorMessage());
		}
		return std::nullopt;
	}

	if (outfile.Active())
	{
		SetWriteBuffer(outfile, 0);
	}

	return paths;
}

inline uint32 ArticleWriter::ProcessArticles(DiskFile& outfile,
							std::string_view infoFilename,
							std::string_view finalOutputPath,
							bool directWrite,
							bool cached)
{
	uint32 crc = 0;
	std::unique_ptr<ArticleCache::FlushGuard> flushGuard;

	if (cached)
	{
		flushGuard = std::make_unique<ArticleCache::FlushGuard>(g_ArticleCache->GuardFlush());
	}

	CharBuffer buffer;
	bool firstArticle = true;

	if (!g_Options->GetRawArticle() && !directWrite)
	{
		buffer.Reserve(1024 * 64);
	}

	for (ArticleInfo* pa : m_fileInfo->GetArticles())
	{
		if (pa->GetStatus() != ArticleInfo::aiFinished) continue;

		// 1. Fill gaps (Direct Write)
		if (!g_Options->GetRawArticle() && !directWrite &&
			pa->GetSegmentOffset() > -1 && pa->GetSegmentOffset() > outfile.Position() && outfile.Position() > -1)
		{
			memset(buffer, 0, static_cast<size_t>(buffer.Size()));
			if (!GetSkipDiskWrite())
			{
				while (pa->GetSegmentOffset() > outfile.Position() && outfile.Position() > -1 &&
					outfile.Write(buffer, std::min(pa->GetSegmentOffset() - outfile.Position(), static_cast<int64_t>(buffer.Size()))));
			}
		}

		// 2. Write Content (Cached or Disk)
		if (pa->GetSegmentContent())
		{
			if (!GetSkipDiskWrite())
			{
				if (!outfile.Seek(pa->GetSegmentOffset()))
				{
					m_commitError = true;
					m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError,
						"Could not seek while writing cached article %s",
						infoFilename.data());
					continue;
				}
				if (outfile.Write(pa->GetSegmentContent(), pa->GetSegmentSize()) !=
					pa->GetSegmentSize())
				{
					m_commitError = true;
					m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError,
						"Could not write cached article %s", infoFilename.data());
				}
			}
			pa->DiscardSegment();
		}
		else if (directWrite && pa->GetDupeFallbackRound() > 0 &&
			pa->GetResultFilename() && !GetSkipDiskWrite())
		{
			// Duplicate articles are staged in their per-article temp file even
			// when DirectWrite is enabled.  Copy only after the queue coordinator
			// has accepted the article's final geometry; never let an unvalidated
			// donor overwrite the shared output during download.
			DiskFile infile;
			if (!nzbget_rs_dqh_article_writer_validate_staged_dupe_size(
				FileSystem::FileSize(pa->GetResultFilename()), pa->GetSegmentSize()))
			{
				m_commitError = true;
				m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError,
					"Staged duplicate article has an invalid size for %s", infoFilename.data());
			}
			else if (infile.Open(pa->GetResultFilename(), DiskFile::omRead))
			{
				if (!outfile.Seek(pa->GetSegmentOffset()))
				{
					m_commitError = true;
					m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError,
						"Could not seek while committing staged duplicate article %s",
						infoFilename.data());
					infile.Close();
					continue;
				}
				CharBuffer stagedBuffer(1024 * 64);
				int cnt = 0;
				int64 copied = 0;
				while ((cnt = infile.Read(stagedBuffer, stagedBuffer.Size())) > 0)
				{
					if (outfile.Write(stagedBuffer, cnt) != cnt)
					{
						m_commitError = true;
						m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError,
							"Could not commit staged duplicate article %s", infoFilename.data());
						break;
					}
					copied += cnt;
				}
				if (!nzbget_rs_dqh_article_writer_validate_staged_dupe_size(
					copied, pa->GetSegmentSize()))
				{
					m_commitError = true;
				}
				infile.Close();
			}
			else
			{
				m_commitError = true;
			}
		}
		else if (!g_Options->GetRawArticle() && !directWrite && !GetSkipDiskWrite())
		{
			// Read from partial file and append to outfile
			DiskFile infile;
			if (pa->GetResultFilename() && infile.Open(pa->GetResultFilename(), DiskFile::omRead))
			{
				int64_t cnt = buffer.Size();
				while (cnt == buffer.Size())
				{
					cnt = infile.Read(buffer, buffer.Size());
					outfile.Write(buffer, cnt);
				}
				infile.Close();
			}
			else
			{
				int32_t newFailed = 0, newSuccess = 0;
				nzbget_rs_dqh_article_writer_apply_missing_part_counters(
					m_fileInfo->GetFailedArticles(), m_fileInfo->GetSuccessArticles(),
					&newFailed, &newSuccess);
				m_fileInfo->SetFailedArticles(newFailed);
				m_fileInfo->SetSuccessArticles(newSuccess);
				m_fileInfo->GetNzbInfo()->PrintMessage(
					Message::mkError, "Could not find file %s for %s [%i/%zu]",
					pa->GetResultFilename(), infoFilename.data(),
					pa->GetPartNumber(), m_fileInfo->GetArticles()->size());
			}
		}
		else if (g_Options->GetRawArticle())
		{
			// Move raw files
			NzbgetRsBuf dstBuf = nzbget_rs_dqh_article_writer_format_raw_part_dest(
				finalOutputPath.data(), finalOutputPath.size(), PATH_SEPARATOR, pa->GetPartNumber());
			const char* dstFileName = dstBuf.data ? dstBuf.data : "";
			if (!FileSystem::MoveFile(pa->GetResultFilename(), dstFileName))
			{
				m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError, "Could not move file %s to %s: %s",
					pa->GetResultFilename(), dstFileName, *FileSystem::GetLastErrorMessage());
			}
			nzbget_rs_free(dstBuf);
		}

		// 3. Calculate CRC
		if (m_format == Decoder::efYenc)
		{
			int firstArt = firstArticle ? 1 : 0;
			crc = nzbget_rs_dqh_article_writer_step_file_crc(
				firstArt, crc, pa->GetCrc(), pa->GetSegmentSize(), &firstArt);
			firstArticle = firstArt != 0;
		}
	}

	buffer.Clear();
	return crc;
}

inline bool ArticleWriter::CommitDiskFile(DiskFile& outfile,
						std::string_view tempDestPath,
						std::string_view finalOutputPath,
						bool directWrite)
{
	if (!outfile.Active()) return true;

	bool flushed = outfile.Flush();
	bool closed = outfile.Close();
	int commitAction = nzbget_rs_dqh_article_writer_eval_commit_post_close(
		flushed ? 1 : 0, closed ? 1 : 0, directWrite ? 1 : 0);
	if (commitAction < 0)
	{
		m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError,
			"Could not flush completed output file %s", finalOutputPath.data());
		return false;
	}

	if (commitAction == 1)
	{
		// a failed rename leaves the complete data under its temporary name
		// (as it always has): it is not a data-commit failure, and failing the
		// file would discard data that par-rename/par-check can still use
		if (!FileSystem::MoveFile(tempDestPath.data(), finalOutputPath.data()))
		{
			m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError, "Could not move file %s to %s: %s",
					tempDestPath.data(), finalOutputPath.data(), *FileSystem::GetLastErrorMessage());
		}
	}
	return true;
}

inline bool ArticleWriter::CleanupOldData(bool directWrite,
						std::string_view nzbDestDir,
						std::string_view finalOutputPath)
{
	std::vector<int> dupeFallbackRounds;
	std::vector<const char*> resultFilenames;
	dupeFallbackRounds.reserve(m_fileInfo->GetArticles()->size());
	resultFilenames.reserve(m_fileInfo->GetArticles()->size());
	for (ArticleInfo* pa : m_fileInfo->GetArticles())
	{
		dupeFallbackRounds.push_back(pa->GetDupeFallbackRound());
		resultFilenames.push_back(pa->GetResultFilename());
	}

	auto sameFilenameCb = [](void*, const char* a, const char* b) -> int {
		return FileSystem::SameFilename(a, b) ? 1 : 0;
	};
	auto fileExistsCb = [](void*, const char* path) -> int {
		return FileSystem::FileExists(path) ? 1 : 0;
	};
	CString moveErrorMessage;
	auto deleteFileCb = [](void*, const char* path) -> int {
		return FileSystem::DeleteFile(path) ? 1 : 0;
	};
	auto moveFileCb = [](void* ctx, const char* from, const char* to) -> int {
		if (FileSystem::MoveFile(from, to))
		{
			return 1;
		}
		if (ctx)
		{
			*static_cast<CString*>(ctx) = FileSystem::GetLastErrorMessage();
		}
		return 0;
	};
	auto dirEmptyCb = [](void*, const char* dir) -> int {
		return FileSystem::DirEmpty(dir) ? 1 : 0;
	};
	auto removeDirCb = [](void*, const char* dir) -> int {
		return FileSystem::RemoveDirectory(dir) ? 1 : 0;
	};

	NzbgetRsBuf checkedOldDirBuf{};
	NzbgetRsBuf deletedOldDirBuf{};
	int moveFailed = 0;
	int hasCheckedOldDir = 0;
	int hasDeletedOldDir = 0;
	int clearOutputFilename = 0;
	nzbget_rs_awc_article_writer_execute_cleanup_old_data(
		directWrite ? 1 : 0,
		nzbDestDir.data(),
		nzbDestDir.size(),
		finalOutputPath.data(),
		finalOutputPath.size(),
		m_outputFilename.data(),
		m_outputFilename.size(),
		m_fileInfo->IsHardLinked() ? 1 : 0,
		Util::EmptyStr(g_Options->GetInterDir()) ? 1 : 0,
		PATH_SEPARATOR,
		ALT_PATH_SEPARATOR,
		dupeFallbackRounds.data(),
		resultFilenames.data(),
		dupeFallbackRounds.size(),
		&moveErrorMessage,
		sameFilenameCb,
		fileExistsCb,
		deleteFileCb,
		moveFileCb,
		dirEmptyCb,
		removeDirCb,
		&moveFailed,
		&checkedOldDirBuf,
		&hasCheckedOldDir,
		&deletedOldDirBuf,
		&hasDeletedOldDir,
		&clearOutputFilename);

	std::string checkedOldDir(checkedOldDirBuf.data ? checkedOldDirBuf.data : "", checkedOldDirBuf.len);
	nzbget_rs_free(checkedOldDirBuf);
	std::string deletedOldDir(deletedOldDirBuf.data ? deletedOldDirBuf.data : "", deletedOldDirBuf.len);
	nzbget_rs_free(deletedOldDirBuf);

	if (moveFailed)
	{
		m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError, "Could not move file %s to %s: %s",
			m_outputFilename.c_str(), finalOutputPath.data(), *moveErrorMessage);
	}
	if (hasCheckedOldDir)
	{
		debug("Checking old dir for: %s", m_outputFilename.c_str());
	}
	if (hasDeletedOldDir)
	{
		debug("Deleting old dir: %s", deletedOldDir.c_str());
	}
	if (clearOutputFilename)
	{
		m_fileInfo->SetOutputFilename(nullptr);
	}
	return true;
}

inline void ArticleWriter::ReportCompletionStatus(std::string_view infoFilename)
{
	const int completionKind = nzbget_rs_awc_article_writer_classify_completion_status(
		m_fileInfo->GetTotalArticles(), m_fileInfo->GetSuccessArticles(),
		m_fileInfo->GetMissedArticles(), m_fileInfo->GetFailedArticles());
	if (completionKind == 1)
	{
		m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkInfo, "Successfully downloaded %s", infoFilename.data());
	}
	else if (completionKind == 2)
	{
		m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkWarning, "%i of %i article downloads failed for \"%s\"",
			nzbget_rs_dqh_article_writer_calc_total_failed_articles(
				m_fileInfo->GetMissedArticles(), m_fileInfo->GetFailedArticles()),
			m_fileInfo->GetTotalArticles(), infoFilename.data());
	}
	else
	{
		m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkInfo, "Partially downloaded %s", infoFilename.data());
	}
}

inline void ArticleWriter::HandlePostProcessing(uint32 crc,
								std::string_view currentPath,
								std::string_view originalFilename,
								std::string_view nzbDestDir)
{
	GuardedDownloadQueue guard = DownloadQueue::Guard();
	m_fileInfo->SetCrc(crc);
	m_fileInfo->SetOutputFilename(currentPath.data());

	// 1. Rename if file was renamed during completion
	if (nzbget_rs_dqh_article_writer_was_renamed_during_completion(
		m_fileInfo->GetFilename(), originalFilename.data(), originalFilename.size()) != 0)
	{
		CString newOfn = FileSystem::MakeUniqueFilename(nzbDestDir.data(), m_fileInfo->GetFilename());
		if (!FileSystem::MoveFile(m_fileInfo->GetOutputFilename().c_str(), *newOfn))
		{
			m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError, "Could not rename file %s to %s: %s",
				m_fileInfo->GetOutputFilename().c_str(), *newOfn, *FileSystem::GetLastErrorMessage());
		}
		m_fileInfo->SetOutputFilename(newOfn);
	}

	// 2. Handle Hardlinks
	if (nzbget_rs_dqh_article_writer_should_delete_hardlinked_without_interdir(
		m_fileInfo->IsHardLinked() ? 1 : 0,
		Util::EmptyStr(g_Options->GetInterDir()) ? 1 : 0) != 0)
	{
		if (FileSystem::DeleteFile(m_outputFilename.c_str()))
		{
			m_fileInfo->SetOutputFilename(nullptr);
			return;
		}
		else
		{
			m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError, "Cannot remove hardlink");
		}
	}

	// 3. Move Completed Files (if destination changed)
	if (nzbget_rs_dqh_article_writer_did_dest_dir_change(
		m_fileInfo->GetNzbInfo()->GetDestDir(), nzbDestDir.data(), nzbDestDir.size()) != 0)
	{
		MoveCompletedFiles(m_fileInfo->GetNzbInfo(), nzbDestDir.data());
	}
}

inline void ArticleWriter::FlushCache()
{
	detail("Flushing cache for %s", m_infoName.c_str());

	// a file being retried writes into its existing output (ForceDirectWrite), as
	// Start and Finish do: flushed to temporary files instead, its cached articles
	// were never written to the file although marked done
	bool directWrite = (g_Options->GetDirectWrite() || m_fileInfo->GetForceDirectWrite()) &&
		m_fileInfo->GetOutputInitialized();
	DiskFile outfile;
	bool needBufFile = false;
	int flushedArticles = 0;
	int64 flushedSize = 0;

	{
		ArticleCache::FlushGuard flushGuard = g_ArticleCache->GuardFlush();

		std::vector<ArticleInfo*> cachedArticles;

		{
			Guard contentGuard = g_ArticleCache->GuardContent();

			if (m_fileInfo->GetFlushLocked())
			{
				return;
			}

			m_fileInfo->SetFlushLocked(true);

			cachedArticles.reserve(m_fileInfo->GetArticles()->size());
			for (ArticleInfo* pa : m_fileInfo->GetArticles())
			{
				if (pa->GetSegmentContent())
				{
					cachedArticles.push_back(pa);
				}
			}
		}

		for (ArticleInfo* pa : cachedArticles)
		{
			if (m_fileInfo->GetDeleted() && !m_fileInfo->GetNzbInfo()->GetParking())
			{
				// the file was deleted during flushing: stop flushing immediately
				break;
			}

			if (!GetSkipDiskWrite())
			{
				if (directWrite && !outfile.Active())
				{
					const std::string& outputFilename = m_fileInfo->GetOutputFilename();
					if (!outfile.Open(outputFilename.c_str(), DiskFile::omReadWrite))
					{
						m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError,
							"Could not open file %s: %s", outputFilename.c_str(),
							*FileSystem::GetLastErrorMessage());
						// prevent multiple error messages
						pa->DiscardSegment();
						flushedArticles++;
						break;
					}
					needBufFile = true;
				}

				BString<1024> destFile;

				if (!directWrite)
				{
					NzbgetRsBuf tmpBuf = nzbget_rs_tpr_article_writer_format_tmp_path(pa->GetResultFilename());
					destFile.Set(tmpBuf.data ? tmpBuf.data : "", (int)tmpBuf.len);
					nzbget_rs_free(tmpBuf);
					if (!outfile.Open(destFile, DiskFile::omWrite))
					{
						m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError,
							"Could not create file %s: %s", *destFile,
							*FileSystem::GetLastErrorMessage());
						// prevent multiple error messages
						pa->DiscardSegment();
						flushedArticles++;
						break;
					}
					needBufFile = true;
				}

				if (outfile.Active() && needBufFile)
				{
					SetWriteBuffer(outfile, 0);
					needBufFile = false;
				}

				if (directWrite)
				{
					outfile.Seek(pa->GetSegmentOffset());
				}

				outfile.Write(pa->GetSegmentContent(), pa->GetSegmentSize());

				flushedSize += pa->GetSegmentSize();
				flushedArticles++;

				pa->DiscardSegment();

				if (!directWrite)
				{
					outfile.Close();

					if (!FileSystem::MoveFile(destFile, pa->GetResultFilename()))
					{
						m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError,
							"Could not rename file %s to %s: %s", *destFile, pa->GetResultFilename(),
							*FileSystem::GetLastErrorMessage());
					}
				}
			}
			else
			{
				flushedSize += pa->GetSegmentSize();
				flushedArticles++;
				pa->DiscardSegment();
			}
		}

		outfile.Close();

		{
			Guard contentGuard = g_ArticleCache->GuardContent();
			m_fileInfo->SetCachedArticles(m_fileInfo->GetCachedArticles() - flushedArticles);
			m_fileInfo->SetFlushLocked(false);
		}
	}

	double flushedMb = nzbget_rs_dqh_stream_repair_bytes_to_mb(flushedSize);
	if (GetSkipDiskWrite())
	{
		detail("Discarded %i articles (%.2f MB) from cache (SkipWrite) for %s", flushedArticles,
			flushedMb, m_infoName.c_str());
	}
	else
	{
		detail("Saved %i articles (%.2f MB) from cache into disk for %s", flushedArticles,
			flushedMb, m_infoName.c_str());
	}
}

inline bool ArticleWriter::MoveCompletedFiles(NzbInfo* nzbInfo, const char* oldDestDir)
{
	if (nzbInfo->GetSkipDiskWrite())
	{
		return true; // nothing was written, nothing to move
	}

	if (nzbInfo->GetCompletedFiles()->empty())
	{
		return true;
	}

	const fs::path destDir = fs::u8path(nzbInfo->GetDestDir());
	const fs::path oldDir  = fs::u8path(oldDestDir);
	fs::error_code ec;
	fs::create_directories(destDir, ec);
	if (ec)
	{
		nzbInfo->PrintMessage(
			Message::mkError,
			"Could not create directory %s: %s",
			nzbInfo->GetDestDir(),
			ec.message().c_str()
		);
		return false;
	}

	// move already downloaded files to new destination
	for (CompletedFile& completedFile : nzbInfo->GetCompletedFiles())
	{
		const auto fileName = fs::u8path(completedFile.GetFilename());
		const auto oldPath = oldDir / fileName;
		fs::path newPath = destDir / fileName;

		if (oldPath == newPath || !fs::exists(oldPath, ec)) continue;

		if (fs::exists(newPath, ec))
		{
			newPath = fs::make_unique_filename(newPath);
		}

		if (newPath.has_parent_path())
		{
			fs::create_directories(newPath.parent_path(), ec);
		}

		fs::move_file(oldPath, newPath, ec);
		if (ec)
		{
			const auto newPathStr = fs::u8string(newPath);
			nzbInfo->PrintMessage(Message::mkError, "Could not move file %s to %s: %s",
				oldPath.string().c_str(), newPathStr.c_str(), ec.message().c_str());
		}
		else
		{
			fs::path oldDirParent = oldDir.parent_path();
			fs::path parent = oldPath.parent_path();
			while (parent != oldDirParent)
			{
				if (fs::exists(parent, ec) && fs::is_empty(parent, ec))
				{
					fs::remove(parent, ec);
					parent = parent.parent_path();
				}
				else
				{
					break;
				}
			}
		}
	}

	if (fs::exists(oldDir, ec) && fs::is_empty(oldDir, ec))
	{
		bool pendingWrites = false;
		for (FileInfo* fileInfo : nzbInfo->GetFileList())
		{
			if (pendingWrites) break;

			if (fileInfo->GetOutputInitialized() && !fileInfo->GetOutputFilename().empty() &&
				fileInfo->GetActiveDownloads() > 0)
			{
				Guard guard = fileInfo->GuardOutputFile();
				if (fileInfo->GetOutputInitialized())
				{
					pendingWrites = true;
				}
			}
		}

		if (!pendingWrites)
		{
			fs::remove(oldDir, ec);
		}
	}

	return true;
}

inline CachedSegmentData ArticleCache::Alloc(int size)
{
	std::lock_guard<std::mutex> guard(m_allocMutex);

	void* p = nullptr;
	size_t effectiveLimit = g_MemoryMonitor ?
		g_MemoryMonitor->GetEffectiveArticleCacheBytes() :
		nzbget_rs_tpr_article_cache_mb_to_bytes(g_Options->GetArticleCache());

	int wasEmpty = 0;
	if (nzbget_rs_awc_article_cache_can_alloc(effectiveLimit, m_allocated, (size_t)size, &wasEmpty) != 0)
	{
		p = malloc(size);
		if (p)
		{
			if (nzbget_rs_dqh_article_cache_should_write_cache_flag(
				wasEmpty, g_Options->GetServerMode() ? 1 : 0,
				g_Options->GetContinuePartial() ? 1 : 0,
				g_Options->GetSkipWrite() ? 1 : 0) != 0)
			{
				g_DiskState->WriteCacheFlag();
			}
			if (wasEmpty)
			{
				// Resume Run(), the notification arrives later, after releasing m_allocMutex
				m_allocCond.notify_all();
			}
			m_allocated += size;
		}
	}

	return CachedSegmentData((char*)p, p ? size : 0);
}

inline bool ArticleCache::Realloc(CachedSegmentData* segment, int newSize)
{
	std::lock_guard<std::mutex> guard(m_allocMutex);

	void* p = realloc(segment->m_data, newSize);
	if (p)
	{
		m_allocated += newSize - segment->m_size;
		segment->m_size = newSize;
		segment->m_data = (char*)p;
	}

	return p;
}

inline void ArticleCache::Free(CachedSegmentData* segment)
{
	if (segment->m_size)
	{
		free(segment->m_data);

		std::lock_guard<std::mutex> guard(m_allocMutex);
		size_t sub = segment->m_size > 0 ? (size_t)segment->m_size : 0;
		m_allocated = m_allocated >= sub ? (m_allocated - sub) : 0;
		if (m_allocated == 0 && g_Options->GetServerMode() && g_Options->GetContinuePartial())
		{
			g_DiskState->DeleteCacheFlag();
		}
	}
}

inline void ArticleCache::Run()
{
	int resetCounter = 0;
	bool justFlushed = false;
	while (nzbget_rs_dqh_article_cache_should_continue_run_loop(
		IsStopped() ? 1 : 0, m_allocated) != 0)
	{
		size_t effectiveLimit = g_MemoryMonitor ?
			g_MemoryMonitor->GetEffectiveArticleCacheBytes() :
			nzbget_rs_tpr_article_cache_mb_to_bytes(g_Options->GetArticleCache());

		int tier = g_MemoryMonitor ? (int)g_MemoryMonitor->GetTier() : (int)NZBGET_RS_PRESSURE_NORMAL;
		int flushEverything = 0;
		int needEmergencyFlush = 0;
		if (nzbget_rs_qhp_article_cache_should_flush(
				m_allocated, effectiveLimit, tier, justFlushed ? 1 : 0, resetCounter,
				IsStopped() ? 1 : 0, g_Options->GetDirectWrite() ? 1 : 0,
				&flushEverything, &needEmergencyFlush) != 0)
		{
			justFlushed = CheckFlush(flushEverything != 0);
			resetCounter = 0;

			int postFlushAction = nzbget_rs_dqh_article_cache_post_flush_action(
				needEmergencyFlush, m_allocated, justFlushed ? 1 : 0, IsStopped() ? 1 : 0);
			if (postFlushAction == 1)
			{
				MemoryMonitor::TrimHeap();
			}
			else if (postFlushAction == 2)
			{
				Util::Sleep(5);
			}
		}
		else if (!m_allocated)
		{
			std::unique_lock<std::mutex> lk(m_allocMutex);
			m_allocCond.wait(lk, [&] { return IsStopped() || m_allocated > 0; });
			resetCounter = 0;
		}
		else
		{
			Util::Sleep(5);
			resetCounter += 5;
		}
	}
}

inline void ArticleCache::Stop()
{
	Thread::Stop();

	// Resume Run() to exit it
	std::lock_guard<std::mutex> guard(m_allocMutex);
	m_allocCond.notify_all();
}

inline bool ArticleCache::CheckFlush(bool flushEverything)
{
	debug("Checking cache, Allocated: %i, FlushEverything: %i", (int)m_allocated, (int)flushEverything);

	BString<1024> infoName;

	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
		for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
		{
			if (m_fileInfo)
			{
				break;
			}

			for (FileInfo* fileInfo : nzbInfo->GetFileList())
			{
				if (nzbget_rs_awc_article_cache_can_flush_file(
						fileInfo->GetCachedArticles(), fileInfo->GetActiveDownloads(),
						fileInfo->GetNzbInfo()->GetSkipDiskWrite() ? 1 : 0,
						flushEverything ? 1 : 0) != 0)
				{
					m_fileInfo = fileInfo;
					NzbgetRsBuf infoBuf = nzbget_rs_dqh_article_writer_format_info_path(
						m_fileInfo->GetNzbInfo()->GetName(), PATH_SEPARATOR, m_fileInfo->GetFilename());
					infoName.Set(infoBuf.data ? infoBuf.data : "", (int)infoBuf.len);
					nzbget_rs_free(infoBuf);
					break;
				}
			}
		}
	}

	if (m_fileInfo)
	{
		ArticleWriter articleWriter;
		articleWriter.SetFileInfo(m_fileInfo);
		articleWriter.SetInfoName(infoName);
		articleWriter.FlushCache();
		m_fileInfo = nullptr;
		return true;
	}

	debug("Checking cache... nothing to flush");

	return false;
}

inline ArticleCache::FlushGuard::FlushGuard(Mutex& mutex) : m_guard(&mutex)
{
	g_ArticleCache->m_flushing = true;
}

inline ArticleCache::FlushGuard::~FlushGuard()
{
	if (m_guard)
	{
		g_ArticleCache->m_flushing = false;
	}
}


inline MemoryMonitor::MemoryMonitor()
{
	InitOptions();
}

inline void MemoryMonitor::InitOptions()
{
	if (g_Options && !Util::EmptyStr(g_Options->GetMemoryPressurePath()))
	{
		nzbget_rs_memory_pressure_set_mock_root(g_Options->GetMemoryPressurePath());
	}
	else if (g_Options)
	{
		nzbget_rs_memory_pressure_set_mock_root(nullptr);
	}
	UpdateState();
}

inline MemoryMonitor::~MemoryMonitor()
{
	if (IsRunning())
	{
		Stop();
	}
	while (IsRunning())
	{
		Util::Sleep(10);
	}
}

inline void MemoryMonitor::Run()
{
	debug("MemoryMonitor thread started");

	while (!IsStopped())
	{
		UpdateState();

		int interval = g_Options ? g_Options->GetMemoryPressureInterval() : 500;
		if (interval < 100) interval = 100;

		int slept = 0;
		while (nzbget_rs_dqh_memory_monitor_should_keep_sleeping(
			IsStopped() ? 1 : 0, slept, interval) != 0)
		{
			Util::Sleep(50);
			slept += 50;
		}
	}

	debug("MemoryMonitor thread stopped");
}

inline void MemoryMonitor::Stop()
{
	Thread::Stop();
}

inline void MemoryMonitor::UpdateState()
{
	int rawSample = nzbget_rs_memory_pressure_sample();
	int currentTier = m_tier.load(std::memory_order_relaxed);
	int nextTier = currentTier;
	bool transitioned = nzbget_rs_qhp_memory_monitor_step(
		rawSample, currentTier, &m_candidateTier, &m_consecutiveSamples, &nextTier) != 0;

	uint32_t cfgCache = g_Options ? (uint32_t)g_Options->GetArticleCache() : 0;
	uint32_t cfgPar = g_Options ? (uint32_t)g_Options->GetParBuffer() : 0;
	uint32_t cfgLimit = 20;
	uint32_t rsEffCache = (nextTier != NZBGET_RS_PRESSURE_CRITICAL &&
		nextTier != NZBGET_RS_PRESSURE_NORMAL && rawSample == nextTier) ?
		nzbget_rs_memory_pressure_effective_cache(cfgCache) : 0;
	uint32_t rsEffPar = (nextTier != NZBGET_RS_PRESSURE_CRITICAL &&
		nextTier != NZBGET_RS_PRESSURE_NORMAL && rawSample == nextTier) ?
		nzbget_rs_memory_pressure_effective_par_buffer(cfgPar) : 0;
	uint32_t effCache = 0, effPar = 0, effLimit = 0;
	nzbget_rs_awc_memory_monitor_resolve_tier_limits(
		nextTier,
		rawSample,
		cfgCache,
		cfgPar,
		cfgLimit,
		(int)NZBGET_RS_PRESSURE_NORMAL,
		(int)NZBGET_RS_PRESSURE_CONSTRAINED,
		(int)NZBGET_RS_PRESSURE_CRITICAL,
		rsEffCache,
		rsEffPar,
		&effCache,
		&effPar,
		&effLimit);
	m_effectiveCacheMB.store(effCache, std::memory_order_release);
	m_effectiveParBufferMB.store(effPar, std::memory_order_release);
	m_effectiveDownloadsLimit.store(effLimit, std::memory_order_release);

	if (transitioned)
	{
		m_tier.store(nextTier, std::memory_order_release);
		ApplyTierTransition((NzbgetRsPressureTier)currentTier, (NzbgetRsPressureTier)nextTier);
	}
}

inline void MemoryMonitor::ApplyTierTransition(NzbgetRsPressureTier oldTier, NzbgetRsPressureTier newTier)
{
	int action = nzbget_rs_awc_memory_monitor_classify_tier_transition_action(
		(int)oldTier, (int)newTier,
		(int)NZBGET_RS_PRESSURE_NORMAL,
		(int)NZBGET_RS_PRESSURE_CONSTRAINED,
		(int)NZBGET_RS_PRESSURE_CRITICAL);
	if (action == 1)
	{
		warn("System memory pressure CRITICAL (>=90%%): shedding article cache and throttling downloads");
		if (g_ArticleCache)
		{
			g_ArticleCache->WakeupFlush();
		}
		TrimHeap();
	}
	else if (action == 2)
	{
		detail("System memory pressure CONSTRAINED: shrinking article cache ceiling");
		if (g_ArticleCache)
		{
			g_ArticleCache->WakeupFlush();
		}
	}
	else if (action == 3)
	{
		detail("System memory pressure returned to NORMAL: restoring cache and download capacity");
		TrimHeap();
	}
}

#endif
