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


#include "nzbget.h"

#include <sstream>
#include <iomanip>
#include "ArticleWriter.h"
#include "DiskState.h"
#include "Options.h"
#include "Log.h"
#include "Util.h"
#include "FileSystem.h"

CachedSegmentData::~CachedSegmentData()
{
	if (g_ArticleCache)
	{
		g_ArticleCache->Free(this);
	}
}

CachedSegmentData& CachedSegmentData::operator=(CachedSegmentData&& other)
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


void ArticleWriter::SetWriteBuffer(DiskFile& outFile, int recSize)
{
	int bufSize = nzbget_rs_qhp_calc_write_buffer_size(g_Options->GetWriteBuffer(), recSize);
	if (bufSize > 0)
	{
		outFile.SetWriteBuffer(bufSize);
	}
}

void ArticleWriter::Prepare()
{
	BuildOutputFilename();
	m_resultFilename = m_articleInfo->GetResultFilename()
		? m_articleInfo->GetResultFilename()
		: "";
}

bool ArticleWriter::IsDupeFallbackArticle() const
{
	return m_articleInfo && m_articleInfo->GetDupeFallbackRound() > 0;
}

bool ArticleWriter::Start(Decoder::EFormat format, const char* filename, int64 fileSize,
	int64 articleOffset, int articleSize)
{
	m_outFile.Close();
	m_format = format;
	m_articleOffset = articleOffset;
	m_articleSize = nzbget_rs_cwp_article_writer_select_article_size(articleSize, m_articleInfo->GetSize());
	m_articlePtr = 0;

	// prepare file for writing
	if (m_format == Decoder::efYenc)
	{
		if (nzbget_rs_dad_article_writer_should_check_dupe_file(
			(int)m_format,
			(int)Decoder::efYenc,
			g_Options->GetDupeCheck() ? 1 : 0,
			(int)m_fileInfo->GetNzbInfo()->GetDupeMode(),
			(int)dmForce,
			m_fileInfo->GetNzbInfo()->GetManyDupeFiles() ? 1 : 0) != 0)
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

			BString<1024> destFilePath("%s%c%s", m_fileInfo->GetNzbInfo()->GetDestDir(), PATH_SEPARATOR, filename);
			bool isOwnOutputFile = false;
			if (g_Options->GetDirectWrite() || m_fileInfo->GetForceDirectWrite())
			{
				Guard guard = m_fileInfo->GuardOutputFile();
				isOwnOutputFile = m_fileInfo->GetForceDirectWrite() ||
					(!m_fileInfo->GetOutputFilename().empty() &&
					 FileSystem::SameFilename(m_fileInfo->GetOutputFilename().c_str(), destFilePath));
			}

			bool destExists = !outputInitialized && !isOwnOutputFile && filename &&
				FileSystem::FileExists(destFilePath);
			if (nzbget_rs_dad_article_writer_is_duplicate_dest(
				outputInitialized ? 1 : 0,
				isOwnOutputFile ? 1 : 0,
				filename ? 1 : 0,
				destExists ? 1 : 0) != 0)
			{
				m_duplicate = true;
				return false;
			}
		}

		if (g_Options->GetDirectWrite() && !IsDupeFallbackArticle())
		{
			Guard guard = m_fileInfo->GuardOutputFile();
			bool outInit = m_fileInfo->GetOutputInitialized();
			bool skipDisk = GetSkipDiskWrite();
			bool outExists = (outInit && !skipDisk) ? FileSystem::FileExists(m_outputFilename.c_str()) : true;
			if (nzbget_rs_aws_article_writer_should_init_direct_write_file(
				1, 0, outInit ? 1 : 0, skipDisk ? 1 : 0, outExists ? 1 : 0) != 0)
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
	if (nzbget_rs_dad_article_writer_can_use_article_cache(
		g_Options->GetArticleCache(),
		g_Options->GetRawArticle() ? 1 : 0,
		(int)m_format,
		(int)Decoder::efYenc,
		IsDupeFallbackArticle() ? 1 : 0,
		isCritical ? 1 : 0) != 0)
	{
		m_articleData = g_ArticleCache->Alloc(m_articleSize);

		while (nzbget_rs_aws_article_writer_should_wait_for_cache_flush(
			m_articleData.GetData() ? 1 : 0,
			g_ArticleCache->GetFlushing() ? 1 : 0,
			(g_MemoryMonitor && g_MemoryMonitor->GetTier() == NZBGET_RS_PRESSURE_CRITICAL) ? 1 : 0) != 0)
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

		bool directWrite = nzbget_rs_cwp_article_writer_is_direct_write_enabled(
			g_Options->GetDirectWrite() ? 1 : 0,
			m_fileInfo->GetForceDirectWrite() ? 1 : 0,
			(int)m_format,
			(int)Decoder::efYenc,
			IsDupeFallbackArticle() ? 1 : 0) != 0;
		const char* outFilename = directWrite ? m_outputFilename.c_str() : m_tempFilename.c_str();
		if (!m_outFile.Open(outFilename, directWrite ? DiskFile::omReadWrite : DiskFile::omWrite))
		{
			m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError,
				"Could not %s file %s: %s", directWrite ? "open" : "create", outFilename,
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

bool ArticleWriter::GetSkipDiskWrite()
{
	bool hasFileAndNzb = m_fileInfo && m_fileInfo->GetNzbInfo();
	return nzbget_rs_cwp_article_writer_should_skip_disk_write(
		hasFileAndNzb ? 1 : 0,
		(hasFileAndNzb && m_fileInfo->GetNzbInfo()->GetSkipDiskWrite()) ? 1 : 0,
		g_Options->GetSkipWrite() ? 1 : 0) != 0;
}

bool ArticleWriter::Write(char* buffer, int len)
{
	if (nzbget_rs_dad_article_writer_advance_and_check_range(
		g_Options->GetRawArticle() ? 1 : 0,
		&m_articlePtr,
		len,
		m_articleSize) != 0)
	{
		return true;
	}

	if (nzbget_rs_aws_article_writer_should_memcpy_cached_segment(
		g_Options->GetRawArticle() ? 1 : 0, m_articleData.GetData() ? 1 : 0) != 0)
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

void ArticleWriter::Finish(bool success)
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

	bool directWrite = nzbget_rs_cwp_article_writer_is_direct_write_enabled(
		g_Options->GetDirectWrite() ? 1 : 0,
		m_fileInfo->GetForceDirectWrite() ? 1 : 0,
		(int)m_format,
		(int)Decoder::efYenc,
		IsDupeFallbackArticle() ? 1 : 0) != 0;

	if (!g_Options->GetRawArticle())
	{
		if (nzbget_rs_aws_article_writer_should_move_temp_to_result(
			directWrite ? 1 : 0, m_articleData.GetData() ? 1 : 0, GetSkipDiskWrite() ? 1 : 0) != 0)
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
			if (nzbget_rs_aws_article_writer_should_realloc_cached_segment(m_articleSize, m_articlePtr) != 0)
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
bool ArticleWriter::CreateOutputFile(int64 size)
{
	if (FileSystem::FileExists(m_outputFilename.c_str()))
	{
		if (FileSystem::FileSize(m_outputFilename.c_str()) == size)
		{
			// keep existing old file from previous program session
			return true;
		}
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

void ArticleWriter::BuildOutputFilename()
{
	if (!m_fileInfo || !m_articleInfo)
		return;

	const char* tmpDir = g_Options->GetTempDir();
	const char* destDir = m_fileInfo->GetNzbInfo()->GetDestDir();
	if (Util::EmptyStr(tmpDir) || Util::EmptyStr(destDir))
		return;

	char fmtBuf[1024];
	size_t fmtLen = nzbget_rs_awc_article_writer_format_result_filename(
		tmpDir, PATH_SEPARATOR, m_fileInfo->GetId(), m_articleInfo->GetPartNumber(),
		fmtBuf, sizeof(fmtBuf));
	std::string filename;
	if (fmtLen < sizeof(fmtBuf))
	{
		filename.assign(fmtBuf, fmtLen);
	}
	else
	{
		filename.resize(fmtLen);
		nzbget_rs_awc_article_writer_format_result_filename(
			tmpDir, PATH_SEPARATOR, m_fileInfo->GetId(), m_articleInfo->GetPartNumber(),
			filename.data(), fmtLen + 1);
	}

	m_articleInfo->SetResultFilename(filename.c_str());
	m_tempFilename = filename + ".tmp";

	if (g_Options->GetDirectWrite() || m_fileInfo->GetForceDirectWrite())
	{
		Guard guard = m_fileInfo->GuardOutputFile();
		const std::string& outputFilename = m_fileInfo->GetOutputFilename();

		if (!outputFilename.empty())
		{
			filename = outputFilename;
		}
		else
		{
			BString<1024> destPath("%s%c%s", destDir, PATH_SEPARATOR, m_fileInfo->GetFilename());
			if (m_fileInfo->GetForceDirectWrite() && m_fileInfo->GetOutputInitialized() && FileSystem::FileExists(destPath))
			{
				filename = destPath.Str();
				m_fileInfo->SetOutputFilename(filename);
			}
			else
			{
				size_t outLen = nzbget_rs_awc_article_writer_format_direct_out_tmp(
					destDir, PATH_SEPARATOR, m_fileInfo->GetId(), fmtBuf, sizeof(fmtBuf));
				if (outLen < sizeof(fmtBuf))
				{
					filename.assign(fmtBuf, outLen);
				}
				else
				{
					filename.resize(outLen);
					nzbget_rs_awc_article_writer_format_direct_out_tmp(
						destDir, PATH_SEPARATOR, m_fileInfo->GetId(), filename.data(), outLen + 1);
				}
				m_fileInfo->SetOutputFilename(filename);
			}
		}

		m_outputFilename = std::move(filename);
	}
 }

bool ArticleWriter::CompleteFileParts()
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
		BString<1024> destPath("%s%c%s", nzbDestDir.c_str(), PATH_SEPARATOR, filename.c_str());
		hasOwnOut = !m_outputFilename.empty() && FileSystem::FileExists(m_outputFilename.c_str());
		hasForceDest = !hasOwnOut && forceDirectWrite && FileSystem::FileExists(destPath);
	}
	int newOutputInit = curOutputInit ? 1 : 0;
	bool directWrite = nzbget_rs_dad_article_writer_resolve_direct_write_ready(
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

	std::string fullInfoPath = nzbName + PATH_SEPARATOR + filename;
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

void ArticleWriter::LogStartMessage(std::string_view infoFilename, bool directWrite, bool cached)
{
	int startMode = nzbget_rs_sap_article_writer_log_start_mode(
		g_Options->GetRawArticle() ? 1 : 0, directWrite ? 1 : 0, cached ? 1 : 0);
	if (startMode == 0)
	{
		detail("Moving articles for %s", infoFilename.data());
	}
	else if (startMode == 1)
	{
		detail("Writing articles for %s", infoFilename.data());
	}
	else if (startMode == 2)
	{
		detail("Checking articles for %s", infoFilename.data());
	}
	else
	{
		detail("Joining articles for %s", infoFilename.data());
	}
}

std::optional<ArticleWriter::OutputPaths>
ArticleWriter::SetupOutputFile(DiskFile &outfile,
							std::string_view destDir,
							std::string_view filename,
							std::string_view infoFilename,
							bool directWrite,
							bool cached)
{
	OutputPaths paths;
	std::string targetPath = std::string(destDir) + PATH_SEPARATOR + std::string(filename);
	if (nzbget_rs_dqh_article_writer_use_target_path_directly(
		m_fileInfo->GetForceDirectWrite() ? 1 : 0,
		directWrite ? 1 : 0,
		m_outputFilename.empty() ? 1 : 0,
		(!m_outputFilename.empty() && FileSystem::SameFilename(m_outputFilename.c_str(), targetPath.c_str())) ? 1 : 0))
	{
		paths.finalPath = targetPath;
	}
	else
	{
		paths.finalPath = FileSystem::MakeUniqueFilename(destDir.data(), filename.data()).Str();
	}

	paths.tempPath = paths.finalPath + ".tmp";

	// a name with a folder (DirectRename gives a file in progress the relative
	// path par2 lists, "Sub/file"): the folder may not exist yet, and both the
	// move of the temporary file and the creation of the joined file failed
	if (filename.find(PATH_SEPARATOR) != std::string_view::npos)
	{
		CString errmsg;
		std::string parentDir = paths.finalPath.substr(0, paths.finalPath.rfind(PATH_SEPARATOR));
		if (!FileSystem::ForceDirectories(parentDir.c_str(), errmsg))
		{
			m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError, "Could not create directory %s: %s",
				parentDir.c_str(), *errmsg);
		}
	}

	int setupScenario = nzbget_rs_aws_article_writer_classify_setup_output_scenario(
		g_Options->GetRawArticle() ? 1 : 0, directWrite ? 1 : 0);
	// Scenario A: Standard Assembly (Write to .tmp)
	if (setupScenario == 1)
	{
		FileSystem::DeleteFile(paths.tempPath.c_str());
		if (!outfile.Open(paths.tempPath.c_str(), DiskFile::omWrite))
		{
			m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError, "Could not create file %s: %s", paths.tempPath.c_str(), *FileSystem::GetLastErrorMessage());
			return std::nullopt;
		}
	}
	// Scenario B: Direct Write (Append/Modify existing)
	else if (setupScenario == 2)
	{
		const char* openPath = m_outputFilename.c_str();
		if (!FileSystem::FileExists(openPath) && FileSystem::FileExists(paths.finalPath.c_str()))
		{
			openPath = paths.finalPath.c_str();
			m_outputFilename = paths.finalPath;
		}

		if (!outfile.Open(openPath, DiskFile::omReadWrite))
		{
			m_fileInfo->GetNzbInfo()->PrintMessage(
				Message::mkError, "Could not open file %s: %s",
				openPath, *FileSystem::GetLastErrorMessage());
			return std::nullopt;
		}
		paths.tempPath = m_outputFilename;
	}
	// Scenario C: Raw Mode (Directory creation)
	else if (setupScenario == 3)
	{
		FileSystem::DeleteFile(paths.tempPath.c_str());
		if (!FileSystem::CreateDirectory(paths.finalPath.c_str()))
		{
			m_fileInfo->GetNzbInfo()->PrintMessage(
				Message::mkError, "Could not create directory %s: %s",
				paths.finalPath.c_str(), *FileSystem::GetLastErrorMessage());
			return std::nullopt;
		}
	}

	if (outfile.Active())
	{
		SetWriteBuffer(outfile, 0);
	}

	return paths;
}

uint32 ArticleWriter::ProcessArticles(DiskFile& outfile,
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
		if (nzbget_rs_dad_article_writer_need_gap_fill(
			g_Options->GetRawArticle() ? 1 : 0,
			directWrite ? 1 : 0,
			pa->GetSegmentOffset(),
			outfile.Position()) != 0)
		{
			memset(buffer, 0, static_cast<size_t>(buffer.Size()));
			if (!GetSkipDiskWrite())
			{
				while (pa->GetSegmentOffset() > outfile.Position() && outfile.Position() > -1 &&
					outfile.Write(buffer, nzbget_rs_cwp_article_writer_calc_gap_fill_chunk(
						pa->GetSegmentOffset(), outfile.Position(), static_cast<int64_t>(buffer.Size()))));
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
		else if (nzbget_rs_aws_article_writer_is_staged_dupe_commit_needed(
			directWrite ? 1 : 0, pa->GetDupeFallbackRound(),
			pa->GetResultFilename() ? 1 : 0, GetSkipDiskWrite() ? 1 : 0) != 0)
		{
			// Duplicate articles are staged in their per-article temp file even
			// when DirectWrite is enabled.  Copy only after the queue coordinator
			// has accepted the article's final geometry; never let an unvalidated
			// donor overwrite the shared output during download.
			DiskFile infile;
			if (FileSystem::FileSize(pa->GetResultFilename()) != pa->GetSegmentSize())
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
				if (copied != pa->GetSegmentSize())
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
		else if (nzbget_rs_aws_article_writer_is_standard_part_append_needed(
			g_Options->GetRawArticle() ? 1 : 0, directWrite ? 1 : 0, GetSkipDiskWrite() ? 1 : 0) != 0)
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
				m_fileInfo->SetFailedArticles(m_fileInfo->GetFailedArticles() + 1);
				m_fileInfo->SetSuccessArticles(m_fileInfo->GetSuccessArticles() - 1);
				m_fileInfo->GetNzbInfo()->PrintMessage(
					Message::mkError, "Could not find file %s for %s [%i/%zu]",
					pa->GetResultFilename(), infoFilename.data(),
					pa->GetPartNumber(), m_fileInfo->GetArticles()->size());
			}
		}
		else if (g_Options->GetRawArticle())
		{
			// Move raw files
			BString<1024> dstFileName("%s%c%03i", finalOutputPath.data(), PATH_SEPARATOR, pa->GetPartNumber());
			if (!FileSystem::MoveFile(pa->GetResultFilename(), dstFileName))
			{
				m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError, "Could not move file %s to %s: %s",
					pa->GetResultFilename(), *dstFileName, *FileSystem::GetLastErrorMessage());
			}
		}

		// 3. Calculate CRC
		if (m_format == Decoder::efYenc)
		{
			crc = firstArticle ? pa->GetCrc() : Crc32::Combine(crc, pa->GetCrc(), static_cast<uint32_t>(pa->GetSegmentSize()));
			firstArticle = false;
		}
	}

	buffer.Clear();
	return crc;
}

bool ArticleWriter::CommitDiskFile(DiskFile& outfile,
						std::string_view tempDestPath,
						std::string_view finalOutputPath,
						bool directWrite)
{
	if (!outfile.Active()) return true;

	bool flushed = outfile.Flush();
	bool closed = outfile.Close();
	if (!flushed || !closed)
	{
		m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError,
			"Could not flush completed output file %s", finalOutputPath.data());
		return false;
	}

	if (!directWrite)
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

bool ArticleWriter::CleanupOldData(bool directWrite,
						std::string_view nzbDestDir,
						std::string_view finalOutputPath)
{
	if (directWrite)
	{
		for (ArticleInfo* pa : m_fileInfo->GetArticles())
		{
			if (pa->GetDupeFallbackRound() > 0 && pa->GetResultFilename())
			{
				FileSystem::DeleteFile(pa->GetResultFilename());
			}
		}
		if (!finalOutputPath.empty())
		{
			bool sameFilename = FileSystem::SameFilename(m_outputFilename.c_str(), finalOutputPath.data());
			// a failed rename is logged only, see CommitDiskFile
			if (!sameFilename && !FileSystem::MoveFile(m_outputFilename.c_str(), finalOutputPath.data()))
			{
				m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkError, "Could not move file %s to %s: %s",
					m_outputFilename.c_str(), finalOutputPath.data(), *FileSystem::GetLastErrorMessage());
			}
		}
		else
		{
			if (FileSystem::FileExists(m_outputFilename.c_str()))
			{
				FileSystem::DeleteFile(m_outputFilename.c_str());
			}
		}

		// Clean up old directory if empty
		if (nzbget_rs_awc_article_writer_is_within_dest_dir(
				nzbDestDir.data(), nzbDestDir.size(),
				m_outputFilename.c_str(), m_outputFilename.size(),
				PATH_SEPARATOR, ALT_PATH_SEPARATOR) == 0)
		{
			debug("Checking old dir for: %s", m_outputFilename.c_str());
			BString<1024> oldDestDir;
			oldDestDir.Set(m_outputFilename.c_str(), static_cast<int>(FileSystem::BaseFileName(m_outputFilename.c_str()) - m_outputFilename.c_str()));
			if (FileSystem::DirEmpty(oldDestDir))
			{
				debug("Deleting old dir: %s", *oldDestDir);
				FileSystem::RemoveDirectory(oldDestDir);
			}
		}
	}
	else
	{
		// Delete partial files
		for (ArticleInfo* pa : m_fileInfo->GetArticles())
		{
			if (pa->GetResultFilename())
			{
				FileSystem::DeleteFile(pa->GetResultFilename());
			}
		}
	}

	if (nzbget_rs_aws_article_writer_should_cleanup_hardlinked_temp(
		finalOutputPath.empty() ? 1 : 0, m_fileInfo->IsHardLinked() ? 1 : 0,
		Util::EmptyStr(g_Options->GetInterDir()) ? 1 : 0) != 0)
	{
		if (FileSystem::DeleteFile(m_outputFilename.c_str()))
		{
			m_fileInfo->SetOutputFilename(nullptr);
		}
	}
	return true;
}

void ArticleWriter::ReportCompletionStatus(std::string_view infoFilename)
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
			m_fileInfo->GetMissedArticles() + m_fileInfo->GetFailedArticles(), m_fileInfo->GetTotalArticles(), infoFilename.data());
	}
	else
	{
		m_fileInfo->GetNzbInfo()->PrintMessage(Message::mkInfo, "Partially downloaded %s", infoFilename.data());
	}
}

void ArticleWriter::HandlePostProcessing(uint32 crc,
								std::string_view currentPath,
								std::string_view originalFilename,
								std::string_view nzbDestDir)
{
	GuardedDownloadQueue guard = DownloadQueue::Guard();
	m_fileInfo->SetCrc(crc);
	m_fileInfo->SetOutputFilename(currentPath.data());

	// 1. Rename if file was renamed during completion
	if (m_fileInfo->GetFilename() != originalFilename)
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
	if (m_fileInfo->IsHardLinked() && Util::EmptyStr(g_Options->GetInterDir()))
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
	if (m_fileInfo->GetNzbInfo()->GetDestDir() != nzbDestDir)
	{
		MoveCompletedFiles(m_fileInfo->GetNzbInfo(), nzbDestDir.data());
	}
}

void ArticleWriter::FlushCache()
{
	detail("Flushing cache for %s", m_infoName.c_str());

	// a file being retried writes into its existing output (ForceDirectWrite), as
	// Start and Finish do: flushed to temporary files instead, its cached articles
	// were never written to the file although marked done
	bool directWrite = nzbget_rs_sap_article_writer_flush_is_direct_write(
		g_Options->GetDirectWrite() ? 1 : 0,
		m_fileInfo->GetForceDirectWrite() ? 1 : 0,
		m_fileInfo->GetOutputInitialized() ? 1 : 0) != 0;
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
			if (nzbget_rs_sap_article_writer_flush_should_abort_deleted(
				m_fileInfo->GetDeleted() ? 1 : 0,
				m_fileInfo->GetNzbInfo()->GetParking() ? 1 : 0))
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
					destFile.Format("%s.tmp", pa->GetResultFilename());
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

bool ArticleWriter::MoveCompletedFiles(NzbInfo* nzbInfo, const char* oldDestDir)
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
	boost::system::error_code ec;
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

			bool outInit = fileInfo->GetOutputInitialized();
			bool outEmpty = fileInfo->GetOutputFilename().empty();
			int activeDl = fileInfo->GetActiveDownloads();
			bool guardedInit = false;
			if (outInit && !outEmpty && activeDl > 0)
			{
				Guard guard = fileInfo->GuardOutputFile();
				guardedInit = fileInfo->GetOutputInitialized();
			}
			if (nzbget_rs_aws_article_writer_has_pending_file_writes(
				outInit ? 1 : 0, outEmpty ? 1 : 0, activeDl, guardedInit ? 1 : 0) != 0)
			{
				pendingWrites = true;
			}
		}

		if (!pendingWrites)
		{
			fs::remove(oldDir, ec);
		}
	}

	return true;
}

CachedSegmentData ArticleCache::Alloc(int size)
{
	std::lock_guard<std::mutex> guard(m_allocMutex);

	void* p = nullptr;
	size_t effectiveLimit = g_MemoryMonitor ?
		g_MemoryMonitor->GetEffectiveArticleCacheBytes() :
		(size_t)g_Options->GetArticleCache() * 1024 * 1024;

	int wasEmpty = 0;
	if (nzbget_rs_dad_article_cache_can_alloc(effectiveLimit, m_allocated, (size_t)size, &wasEmpty) != 0)
	{
		p = malloc(size);
		if (p)
		{
			if (wasEmpty && g_Options->GetServerMode() && g_Options->GetContinuePartial()
				&& !g_Options->GetSkipWrite())
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

bool ArticleCache::Realloc(CachedSegmentData* segment, int newSize)
{
	std::lock_guard<std::mutex> guard(m_allocMutex);

	void* p = realloc(segment->m_data, newSize);
	if (p)
	{
		m_allocated = (size_t)((ptrdiff_t)m_allocated +
			nzbget_rs_cwp_article_cache_calc_realloc_delta(newSize, segment->m_size));
		segment->m_size = newSize;
		segment->m_data = (char*)p;
	}

	return p;
}

void ArticleCache::Free(CachedSegmentData* segment)
{
	if (segment->m_size)
	{
		free(segment->m_data);

		std::lock_guard<std::mutex> guard(m_allocMutex);
		int deleteCacheFlag = 0;
		m_allocated = nzbget_rs_sap_article_cache_free_eval(
			segment->m_size, m_allocated,
			g_Options->GetServerMode() ? 1 : 0,
			g_Options->GetContinuePartial() ? 1 : 0,
			&deleteCacheFlag);
		if (deleteCacheFlag)
		{
			g_DiskState->DeleteCacheFlag();
		}
	}
}

void ArticleCache::Run()
{
	int resetCounter = 0;
	bool justFlushed = false;
	while (!IsStopped() || m_allocated > 0)
	{
		size_t effectiveLimit = g_MemoryMonitor ?
			g_MemoryMonitor->GetEffectiveArticleCacheBytes() :
			(size_t)g_Options->GetArticleCache() * 1024 * 1024;

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

			if (needEmergencyFlush != 0 && m_allocated == 0)
			{
				MemoryMonitor::TrimHeap();
			}
			else if (!justFlushed && !IsStopped())
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

void ArticleCache::Stop()
{
	Thread::Stop();

	// Resume Run() to exit it
	std::lock_guard<std::mutex> guard(m_allocMutex);
	m_allocCond.notify_all();
}

bool ArticleCache::CheckFlush(bool flushEverything)
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
					infoName.Format("%s%c%s", m_fileInfo->GetNzbInfo()->GetName(), PATH_SEPARATOR, m_fileInfo->GetFilename());
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

ArticleCache::FlushGuard::FlushGuard(Mutex& mutex) : m_guard(&mutex)
{
	g_ArticleCache->m_flushing = true;
}

ArticleCache::FlushGuard::~FlushGuard()
{
	if (m_guard)
	{
		g_ArticleCache->m_flushing = false;
	}
}

MemoryMonitor* g_MemoryMonitor = nullptr;

MemoryMonitor::MemoryMonitor()
{
	InitOptions();
}

void MemoryMonitor::InitOptions()
{
	if (g_Options && g_Options->GetMemoryPressurePath() && strlen(g_Options->GetMemoryPressurePath()) > 0)
	{
		nzbget_rs_memory_pressure_set_mock_root(g_Options->GetMemoryPressurePath());
	}
	else if (g_Options)
	{
		nzbget_rs_memory_pressure_set_mock_root(nullptr);
	}
	UpdateState();
}

MemoryMonitor::~MemoryMonitor()
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

void MemoryMonitor::Run()
{
	debug("MemoryMonitor thread started");

	while (!IsStopped())
	{
		UpdateState();

		int interval = g_Options ? g_Options->GetMemoryPressureInterval() : 500;
		interval = nzbget_rs_cwp_memory_monitor_clamp_interval(interval);

		int slept = 0;
		while (!IsStopped() && slept < interval)
		{
			Util::Sleep(50);
			slept += 50;
		}
	}

	debug("MemoryMonitor thread stopped");
}

void MemoryMonitor::Stop()
{
	Thread::Stop();
}

const char* MemoryMonitor::GetTierName() const
{
	return nzbget_rs_awc_memory_monitor_tier_name(
		(int)GetTier(), (int)NZBGET_RS_PRESSURE_CONSTRAINED, (int)NZBGET_RS_PRESSURE_CRITICAL);
}

size_t MemoryMonitor::GetEffectiveArticleCacheBytes() const
{
	return (size_t)m_effectiveCacheMB.load(std::memory_order_acquire) * 1024 * 1024;
}

int MemoryMonitor::GetEffectiveArticleCacheMB() const
{
	return (int)m_effectiveCacheMB.load(std::memory_order_acquire);
}

int MemoryMonitor::GetEffectiveParBufferMB(int configuredMB) const
{
	int tier = m_tier.load(std::memory_order_acquire);
	int mb = 0;
	if (nzbget_rs_dad_memory_monitor_calc_critical_or_normal_par_buffer(
		tier, configuredMB, (int)NZBGET_RS_PRESSURE_CRITICAL, (int)NZBGET_RS_PRESSURE_NORMAL, &mb) != 0)
	{
		return mb;
	}
	return (int)nzbget_rs_memory_pressure_effective_par_buffer((uint32_t)configuredMB);
}

int MemoryMonitor::GetEffectiveDownloadsLimit(int configuredLimit) const
{
	int tier = m_tier.load(std::memory_order_acquire);
	return nzbget_rs_awc_memory_monitor_effective_downloads_limit(
		tier, configuredLimit, (int)NZBGET_RS_PRESSURE_CONSTRAINED, (int)NZBGET_RS_PRESSURE_CRITICAL);
}

void MemoryMonitor::TrimHeap()
{
	nzbget_rs_memory_pressure_trim_heap();
}

void MemoryMonitor::UpdateState()
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
	nzbget_rs_dad_memory_monitor_resolve_tier_limits(
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

void MemoryMonitor::ApplyTierTransition(NzbgetRsPressureTier oldTier, NzbgetRsPressureTier newTier)
{
	int action = nzbget_rs_aws_memory_monitor_classify_tier_transition_action(
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
