/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2016-2017 Andrey Prygunkov <hugbug@users.sourceforge.net>
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

#include "RarRenamer.h"
#include <exception>
#include "nzbget_rs.h"
#include "Log.h"
#include "Util.h"
#include "FileSystem.h"

void RarRenamer::Execute()
{
	NzbgetRsBuf progBuf = nzbget_rs_tpr_rar_renamer_format_exec_progress(*m_infoName);
	m_progressLabel.Set(progBuf.data ? progBuf.data : "", (int)progBuf.len);
	nzbget_rs_free(progBuf);
	m_stageProgress = 0;
	UpdateProgress();

	BuildDirList(m_destDir);

	for (CString& destDir : m_dirList)
	{
		debug("Checking %s", *destDir);
		CheckFiles(destDir);
	}
}

void RarRenamer::BuildDirList(const char* destDir)
{
	m_dirList.push_back(destDir);

	DirBrowser dirBrowser(destDir);

	while (const char* filename = dirBrowser.Next())
	{
		if (!IsStopped())
		{
			NzbgetRsBuf fullFnBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
				destDir, PATH_SEPARATOR, filename);
			BString<1024> fullFilename;
			fullFilename.Set(fullFnBuf.data ? fullFnBuf.data : "", (int)fullFnBuf.len);
			nzbget_rs_free(fullFnBuf);
			if (FileSystem::DirectoryExists(fullFilename))
			{
				BuildDirList(fullFilename);
			}
			else
			{
				m_fileCount++;
			}
		}
	}
}

void RarRenamer::CheckFiles(const char* destDir)
{
	DirBrowser dir(destDir);
	while (const char* filename = dir.Next())
	{
		if (!IsStopped())
		{
			NzbgetRsBuf fullFnBuf = nzbget_rs_tpr_article_writer_format_dest_file_path(
				destDir, PATH_SEPARATOR, filename);
			BString<1024> fullFilename;
			fullFilename.Set(fullFnBuf.data ? fullFnBuf.data : "", (int)fullFnBuf.len);
			nzbget_rs_free(fullFnBuf);

			if (!FileSystem::DirectoryExists(fullFilename))
			{
				NzbgetRsBuf progBuf = nzbget_rs_tpr_rar_renamer_format_file_progress(filename);
				m_progressLabel.Set(progBuf.data ? progBuf.data : "", (int)progBuf.len);
				nzbget_rs_free(progBuf);
				m_stageProgress = nzbget_rs_psp_rar_renamer_calc_stage_progress(m_fileCount, m_curFile);
				UpdateProgress();
				m_curFile++;

				CheckOneFile(fullFilename);
			}
		}
	}

	if (!m_volumes.empty())
	{
		RenameFiles(destDir);
	}

	// the next directory starts afresh: kept, this one's volumes (under the
	// names they had before the rename) were renamed again into each subfolder
	m_volumes.clear();
}

void RarRenamer::CheckOneFile(const char* filename)
{
	if (m_ignoreExt && Util::MatchFileExt(FileSystem::BaseFileName(filename), m_ignoreExt, ",;"))
	{
		return;
	}
	
	RarVolume volume(filename);
	volume.SetPassword(m_password);
	if (volume.Read())
	{
		m_volumes.push_back(std::move(volume));
	}
}

void RarRenamer::RenameFile(const char* srcFilename, const char* destFileName)
{
	PrintMessage(Message::mkInfo, "Renaming %s to %s", FileSystem::BaseFileName(srcFilename), FileSystem::BaseFileName(destFileName));
	if (!FileSystem::MoveFile(srcFilename, destFileName))
	{
		PrintMessage(Message::mkError, "Could not rename %s to %s: %s", srcFilename, destFileName,
			*FileSystem::GetLastErrorMessage());
		return;
	}

	m_renamedCount++;

	// notify about new file name
	RegisterRenamedFile(FileSystem::BaseFileName(srcFilename), FileSystem::BaseFileName(destFileName));
}

void RarRenamer::RenameFiles(const char* destDir)
{
	// rust/src/rarrenamer.rs builds the sets and picks the new names; the
	// file checks, the regular expression and the moves stay here
	std::vector<std::vector<NzbgetRsRarRenFile>> files;
	std::vector<NzbgetRsRarRenVolume> volumes;
	files.reserve(m_volumes.size());
	volumes.reserve(m_volumes.size());
	for (RarVolume& volume : m_volumes)
	{
		files.emplace_back();
		for (RarFile& file : *volume.GetFiles())
		{
			files.back().push_back({file.GetFilename(), strlen(file.GetFilename()), file.GetSize(),
				file.GetSplitBefore(), file.GetSplitAfter()});
		}
		volumes.push_back({volume.GetFilename(), strlen(volume.GetFilename()), volume.GetVersion(),
			volume.GetVolumeNo(), volume.GetMultiVolume(), volume.GetHasNextVolume(), volume.GetNewNaming(),
			files.back().data(), files.back().size()});
	}

	struct Context
	{
		RarRenamer* renamer;
		RegEx regExPart{".*.part([0-9]+)\\.rar$"};
		std::exception_ptr error{};
	} context{this};

	NzbgetRsRarRenEnv env{};
	env.ctx = &context;
	env.fileExists = [](void* ctx, const char* path) -> int
	{
		Context* c = static_cast<Context*>(ctx);
		if (c->error) return 0;
		try
		{
			return FileSystem::FileExists(path) ? 1 : 0;
		}
		catch (...)
		{
			c->error = std::current_exception();
			return 0;
		}
	};
	env.matchPart = [](void* ctx, const char* name, size_t* start, size_t* len) -> int
	{
		Context* c = static_cast<Context*>(ctx);
		if (c->error) return 0;
		try
		{
			if (!c->regExPart.Match(name)) return 0;
			*start = (size_t)c->regExPart.GetMatchStart(1);
			*len = (size_t)c->regExPart.GetMatchLen(1);
			return 1;
		}
		catch (...)
		{
			c->error = std::current_exception();
			return 0;
		}
	};
	env.rename = [](void* ctx, size_t index, const char* dest)
	{
		Context* c = static_cast<Context*>(ctx);
		if (c->error) return;
		try
		{
			c->renamer->RenameFile(c->renamer->m_volumes[index].GetFilename(), dest);
		}
		catch (...)
		{
			c->error = std::current_exception();
		}
	};

	nzbget_rs_rar_rename_files(destDir, volumes.data(), volumes.size(), &env);
	if (context.error)
	{
		std::rethrow_exception(context.error);
	}
}

