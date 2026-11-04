/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2026 Denis <denis@nzbget.com>
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
 *  along with this program. If not, see <https://www.gnu.org/licenses/>.
 */


#include "nzbget.h"
#include "CollectionAnalyzer.h"
#include "FileTypes.h"
#include "Deobfuscation.h"
#include "Util.h"
#include <algorithm>
#include <cctype>

#include "nzbget_rs.h"

namespace CollectionAnalyzer
{
	namespace
	{
		// AnalyzeDirectory's walk: the files (clutter skipped, unknown types
		// sniffed) and whether a disc structure directory is there; false for
		// a directory that isn't one
		bool Walk(const fs::path& dir, std::vector<FileEntry>& files, bool& discFound)
		{
			fs::error_code ec;
			if (!fs::exists(dir, ec) || !fs::is_directory(dir, ec))
			{
				return false;
			}

			for (auto it = fs::recursive_directory_iterator(dir, fs::directory_options::skip_permission_denied, ec);
				it != fs::recursive_directory_iterator();
				it.increment(ec))
			{
				if (ec) break;

				if (it->is_directory(ec))
				{
					std::string dirname = fs::u8string(it->path().filename());
					if (FileTypes::IsClutterDir(dirname))
					{
						it.disable_recursion_pending();
						continue;
					}
					if (FileTypes::IsDiscStructureDir(dirname))
					{
						discFound = true;
						it.disable_recursion_pending();
					}
					continue;
				}

				if (it->is_regular_file(ec))
				{
					const fs::path& entryPath = it->path();
					std::string filename = fs::u8string(entryPath.filename());
					if (FileTypes::IsClutterFile(filename))
					{
						continue;
					}
					fs::error_code sizeEc;
					uintmax_t size = it->file_size(sizeEc);
					if (sizeEc)
					{
						continue;
					}
					FileEntry fe;
					fe.path = entryPath;
					fe.filename = std::move(filename);
					fe.stem = fs::u8string(entryPath.stem());
					fe.ext = fs::u8string(entryPath.extension());
					fe.size = size;

					bool isKnown = !fe.ext.empty() && (
						FileTypes::IsVideoExt(fe.ext) || FileTypes::IsAudioExt(fe.ext) ||
						FileTypes::IsSubtitleExt(fe.ext) || FileTypes::IsNfoExt(fe.ext) ||
						FileTypes::IsBookExt(fe.ext) || FileTypes::IsImageExt(fe.ext) ||
						FileTypes::IsArchiveExt(fe.ext) || FileTypes::IsParityExt(fe.ext) ||
						FileTypes::IsDiscStructureExt(fe.ext) || FileTypes::IsDiscImageExt(fe.ext) ||
						FileTypes::IsDiscDescriptorExt(fe.ext) || FileTypes::IsGenericDiscImageExt(fe.ext));

					if (!isKnown)
					{
						std::string_view sniffed = FileTypes::SniffExtension(fe.path);
						if (!sniffed.empty())
						{
							fe.ext = sniffed;
						}
					}

					files.push_back(std::move(fe));
				}
			}

			return true;
		}

		struct RustEntries
		{
			// Own the UTF-8 conversions until Rust has finished borrowing them.
			std::vector<std::string> paths, renamePrefixes;
			std::vector<NzbgetRsFileEntry> entries;

			explicit RustEntries(const std::vector<FileEntry>& files)
			{
				paths.reserve(files.size());
				renamePrefixes.reserve(files.size());
				entries.reserve(files.size());
				for (const FileEntry& f : files)
				{
					paths.push_back(fs::u8string(f.path));
					// Preserve this standard library's root/separator spelling and
					// Windows drive-relative paths. The appended name is removed.
					std::string prefix = fs::u8string(f.path.parent_path() / fs::u8path("x"));
					if (!prefix.empty()) prefix.pop_back();
					renamePrefixes.push_back(std::move(prefix));
					const auto& path = paths.back();
					const auto& dir = renamePrefixes.back();
					entries.push_back({path.data(), path.size(), dir.data(), dir.size(),
						f.filename.data(), f.filename.size(), f.stem.data(), f.stem.size(),
						f.ext.data(), f.ext.size(), f.size});
				}
			}
		};

		std::string TakeString(NzbgetRsBuf buf)
		{
			auto release = [](NzbgetRsBuf* owned) { nzbget_rs_free(*owned); };
			std::unique_ptr<NzbgetRsBuf, decltype(release)> owner(&buf, release);
			return std::string(buf.data, buf.len);
		}

		struct PlanContext
		{
			const std::vector<FileEntry>& files;
			const char* ignoreExt;
			RenamePlan& plan;
		};
	}

	// rust/src/collection.rs
	AnalysisResult Analyze(const std::vector<FileEntry>& files)
	{
		RustEntries input(files);
		const auto& entries = input.entries;
		std::vector<size_t> subtitles(files.size()), nfos(files.size()), others(files.size());
		NzbgetRsAnalysis a{};
		a.subtitles = subtitles.data();
		a.nfos = nfos.data();
		a.otherFiles = others.data();
		nzbget_rs_collection_analyze(entries.data(), entries.size(), &a);

		AnalysisResult result;
		if (a.mainVideo >= 0) result.mainVideo = files[a.mainVideo];
		if (a.sampleVideo >= 0) result.sampleVideo = files[a.sampleVideo];
		if (a.mainBook >= 0) result.mainBook = files[a.mainBook];
		for (size_t i = 0; i < a.subtitleCount; i++) result.subtitles.push_back(files[subtitles[i]]);
		for (size_t i = 0; i < a.nfoCount; i++) result.nfos.push_back(files[nfos[i]]);
		for (size_t i = 0; i < a.otherCount; i++) result.otherFiles.push_back(files[others[i]]);
		result.isAmbiguousCollection = a.ambiguous != 0;
		result.isDiscStructure = a.discStructure != 0;
		result.hasAudio = a.hasAudio != 0;
		return result;
	}

	AnalysisResult AnalyzeDirectory(const fs::path& dir)
	{
		std::vector<FileEntry> files;
		bool discFound = false;
		if (!Walk(dir, files, discFound))
		{
			return {};
		}
		AnalysisResult res = Analyze(files);
		if (discFound)
		{
			res.isDiscStructure = true;
		}
		return res;
	}

	RenamePlan BuildPlan(const fs::path& dir, std::string_view targetName, const char* ignoreExt)
	{
		RenamePlan plan;
		std::vector<FileEntry> files;
		bool discFound = false;
		Walk(dir, files, discFound);
		RustEntries input(files);
		const auto& entries = input.entries;

		PlanContext context{files, ignoreExt, plan};
		NzbgetRsPlanCallbacks callbacks{};
		callbacks.user = &context;
		callbacks.exists = [](void*, const char* path, size_t len) noexcept -> int
		{
			fs::error_code ec;
			// a NUL-terminated copy: u8path's conversion on Windows reads to a NUL
			return fs::exists(fs::u8path(std::string(path, len)), ec);
		};
		callbacks.ignored = [](void* user, const char* path, size_t len) noexcept -> int
		{
			const char* ignoreExt = static_cast<PlanContext*>(user)->ignoreExt;
			return ignoreExt && Util::MatchFileExt(std::string(path, len).c_str(), ignoreExt, ",");
		};
		callbacks.action = [](void* user, size_t file, const char* dst, size_t dstLen, const char* name, size_t nameLen) noexcept
		{
			PlanContext* c = static_cast<PlanContext*>(user);
			const FileEntry& entry = c->files[file];
			// the name as the converted path has it (on Windows the UTF-8
			// conversion may change bytes), as the C++ planner reported it
			fs::path dstPath = fs::u8path(std::string(dst, dstLen));
			std::string newFilename = fs::u8string(dstPath.filename());
			(void)name;
			(void)nameLen;
			c->plan.actions.push_back({entry.path, std::move(dstPath), entry.filename, std::move(newFilename)});
		};
		callbacks.stem = [](void*, const char* path, size_t len, char* out, size_t cap) noexcept -> size_t
		{
			// the stem as the converted destination path has it
			std::string stem = fs::u8string(fs::u8path(std::string(path, len)).stem());
			if (stem.size() <= cap)
			{
				memcpy(out, stem.data(), stem.size());
			}
			return stem.size();
		};
		NzbgetRsPlanFlags flags{};
		plan.effectiveBaseName = TakeString(nzbget_rs_collection_plan(entries.data(), entries.size(), discFound,
			targetName.data(), targetName.size(), &callbacks, &flags));
		plan.isAmbiguousCollection = flags.ambiguous != 0;
		plan.isDiscStructure = flags.discStructure != 0;
		plan.canRename = flags.canRename != 0;
		plan.targetNameObfuscated = flags.targetNameObfuscated != 0;
		return plan;
	}

	std::string ResolveTargetName(std::string_view metaName, std::string_view nzbName)
	{
		return TakeString(nzbget_rs_collection_name(0, metaName.data(), metaName.size(), nzbName.data(), nzbName.size(), nullptr, 0));
	}

	std::string ResolveSubtitleName(std::string_view baseName, std::string_view subStem, std::string_view subExt)
	{
		return TakeString(nzbget_rs_collection_name(1, baseName.data(), baseName.size(), subStem.data(), subStem.size(),
			subExt.data(), subExt.size()));
	}

	std::string ResolveSampleName(std::string_view baseName, std::string_view sampleExt)
	{
		return TakeString(nzbget_rs_collection_name(2, baseName.data(), baseName.size(), sampleExt.data(), sampleExt.size(),
			nullptr, 0));
	}
}
