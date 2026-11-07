/*
 *  This file is part of nzbget. See <https://nzbget.com>.
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

#include <vector>
#include "ParDamage.h"
#include "DupeArticleFallback.h"
#include "ParParser.h"
#include "nzbget_rs.h"

void ParDamage::FileCompleted(NzbInfo* nzbInfo, FileInfo* fileInfo, const std::string& filename)
{
	static_assert(sizeof(StreamRange) == sizeof(NzbgetRsByteRange) &&
		offsetof(StreamRange, Offset) == offsetof(NzbgetRsByteRange, offset) &&
		offsetof(StreamRange, Size) == offsetof(NzbgetRsByteRange, size),
		"StreamRange layout must match NzbgetRsByteRange");
	bool isPar = fileInfo->GetParFile() || DupeArticleFallback::IsParFile(fileInfo);
	int failedPlusMissed = fileInfo->GetFailedArticles() + fileInfo->GetMissedArticles();
	int64 decodedSize = fileInfo->GetDecodedFileSize();
	StreamRangeList holes;
	if (!isPar && failedPlusMissed > 0 && decodedSize > 0)
	{
		holes = DupeStreamRepair::ComputeHoles(fileInfo);
	}
	nzbget_rs_par_damage_file_completed(
		nzbInfo->GetId(),
		isPar ? 1 : 0,
		failedPlusMissed,
		decodedSize,
		fileInfo->GetSuccessArticles(),
		fileInfo->GetSize(),
		filename.c_str(),
		holes.empty() ? nullptr : reinterpret_cast<const NzbgetRsByteRange*>(holes.data()),
		holes.size());

	if (isPar && failedPlusMissed == 0 && !filename.empty())
	{
		if (nzbget_rs_streaming_par_has_session(nzbInfo->GetId()) == 0)
		{
			if (nzbget_rs_streaming_par_start(nzbInfo->GetId(), filename.c_str(), nzbInfo->GetDestDir()) != 0)
			{
				for (FileInfo* qFile : nzbInfo->GetFileList())
				{
					if (!qFile || qFile == fileInfo || qFile->GetDeleted())
					{
						continue;
					}
					int baseLen = 0;
					int blocks = 0;
					if (ParParser::ParseParFilename(qFile->GetFilename(), qFile->GetFilenameConfirmed(), &baseLen, &blocks) &&
						blocks != 0)
					{
						nzbget_rs_streaming_par_register_queued_volume(
							nzbInfo->GetId(),
							static_cast<uint64_t>(qFile->GetId()),
							blocks > 0 ? blocks : 1,
							qFile->GetPaused() ? 1 : 0);
					}
				}
			}
		}
		else
		{
			nzbget_rs_streaming_par_on_recovery_volume_arrived(
				nzbInfo->GetId(),
				filename.c_str(),
				static_cast<uint64_t>(fileInfo->GetId()),
				1);
		}
	}
}

int ParDamage::BlocksTouched(const StreamRangeList& holes, uint64 blockSize)
{
	static_assert(sizeof(StreamRange) == sizeof(NzbgetRsByteRange) &&
		offsetof(StreamRange, Offset) == offsetof(NzbgetRsByteRange, offset) &&
		offsetof(StreamRange, Size) == offsetof(NzbgetRsByteRange, size),
		"StreamRange layout must match NzbgetRsByteRange");
	return nzbget_rs_par_damage_blocks_touched(
		holes.empty() ? nullptr : reinterpret_cast<const NzbgetRsByteRange*>(holes.data()),
		holes.size(), blockSize);
}

ParDamage::Verdict ParDamage::Judge(NzbInfo* nzbInfo)
{
	Verdict verdict;
	if (!nzbget_rs_par_damage_has_damage(nzbInfo->GetId()))
	{
		return verdict;
	}

	std::vector<NzbgetRsParDamageQueuedFile> queued;
	queued.reserve(nzbInfo->GetFileList()->size());
	for (FileInfo* fileInfo : nzbInfo->GetFileList())
	{
		queued.push_back({
			fileInfo->GetFilename(),
			fileInfo->GetFilenameConfirmed() ? 1 : 0,
			fileInfo->GetSize()
		});
	}

	std::vector<NzbgetRsParDamageCompletedFile> completed;
	completed.reserve(nzbInfo->GetCompletedFiles()->size());
	for (CompletedFile& completedFile : nzbInfo->GetCompletedFiles())
	{
		completed.push_back({
			completedFile.GetFilename(),
			completedFile.GetStatus() != CompletedFile::cfFailure ? 1 : 0
		});
	}

	NzbgetRsParDamageVerdict rsVerdict{};
	nzbget_rs_par_damage_judge(
		nzbInfo->GetId(),
		nzbInfo->GetDestDir(),
		nzbInfo->GetSize(),
		nzbInfo->GetParSize(),
		queued.empty() ? nullptr : queued.data(),
		queued.size(),
		completed.empty() ? nullptr : completed.data(),
		completed.size(),
		&rsVerdict);

	verdict.known = rsVerdict.known != 0;
	verdict.blockSize = rsVerdict.block_size;
	verdict.damagedBlocks = rsVerdict.damaged_blocks;
	verdict.recoveryBlocks = rsVerdict.recovery_blocks;
	verdict.doneBytes = rsVerdict.done_bytes;
	verdict.totalBytes = rsVerdict.total_bytes;
	verdict.projectedBlocks = rsVerdict.projected_blocks;
	return verdict;
}

bool ParDamage::FirstReport(int nzbId)
{
	return nzbget_rs_par_damage_first_report(nzbId) != 0;
}

void ParDamage::Forget(int nzbId)
{
	nzbget_rs_par_damage_forget(nzbId);
	nzbget_rs_streaming_par_forget(nzbId);
}
