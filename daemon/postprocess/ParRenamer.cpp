/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2013-2016 Andrey Prygunkov <hugbug@users.sourceforge.net>
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


#include "nzbget.h"

#ifndef DISABLE_PARCHECK

#include <par2/libpar2.h>
#include <par2/par2repairer.h>
#include <par2/md5.h>

#include "ParRenamer.h"
#include "ParParser.h"
#include "Log.h"
#include "Options.h"
#include "Util.h"
#include "FileSystem.h"
#include <exception>
#include <memory>
#include "nzbget_rs.h"

class ParRenamerRepairer final : public Par2::Par2Repairer
{
public:
	ParRenamerRepairer() : Par2::Par2Repairer(m_nout, m_nout, Par2::nlNormal) {};
	friend class ParRenamer;
private:
	class NullStreamBuf : public std::streambuf {};
	NullStreamBuf m_nullbuf;
	std::ostream m_nout{&m_nullbuf};
};


void ParRenamer::Execute()
{
	// rust/src/parrenamer.rs; an exception in a callback ends the run and is
	// rethrown here, after the members got what was done until then
	struct Context
	{
		ParRenamer* self;
		std::exception_ptr error;
		int renamedOnError;
		std::unique_ptr<ParRenamerRepairer> repairer;
		std::map<Par2::MD5Hash, Par2::Par2RepairerSourceFile*>::iterator entry;

		int Fail()
		{
			error = std::current_exception();
			return -1;
		}
	} context{this, nullptr, 0, nullptr, {}};

	NzbgetRsParRenamerEnv env{};
	env.ctx = &context;
	env.stopped = [](void* ctx) noexcept -> int
	{
		Context& c = *static_cast<Context*>(ctx);
		try { return c.self->IsStopped() ? 1 : 0; } catch (...) { return c.Fail(); }
	};
	env.progress = [](void* ctx, const char* label, int stage) noexcept -> int
	{
		Context& c = *static_cast<Context*>(ctx);
		try
		{
			c.self->m_progressLabel = label;
			c.self->m_stageProgress = stage;
			c.self->UpdateProgress();
			return 0;
		}
		catch (...) { return c.Fail(); }
	};
	env.message = [](void* ctx, int kind, const char* text) noexcept -> int
	{
		Context& c = *static_cast<Context*>(ctx);
		try
		{
			c.self->PrintMessage(kind == 0 ? Message::mkInfo : kind == 1 ? Message::mkWarning : Message::mkError, "%s", text);
			return 0;
		}
		catch (...) { return c.Fail(); }
	};
	env.debug = [](void* ctx, [[maybe_unused]] const char* text) noexcept -> int
	{
		Context& c = *static_cast<Context*>(ctx);
		try { debug("%s", text); return 0; } catch (...) { return c.Fail(); }
	};
	env.listDir = [](void* ctx, const char* dir, void* sink, void (*add)(void*, const char*)) noexcept -> int
	{
		Context& c = *static_cast<Context*>(ctx);
		try
		{
			DirBrowser dirBrowser(dir);
			while (const char* filename = dirBrowser.Next())
			{
				add(sink, filename);
			}
			return 0;
		}
		catch (...) { return c.Fail(); }
	};
	env.dirExists = [](void* ctx, const char* path) noexcept -> int
	{
		Context& c = *static_cast<Context*>(ctx);
		try { return FileSystem::DirectoryExists(path) ? 1 : 0; } catch (...) { return c.Fail(); }
	};
	env.fileExists = [](void* ctx, const char* path) noexcept -> int
	{
		Context& c = *static_cast<Context*>(ctx);
		try { return FileSystem::FileExists(path) ? 1 : 0; } catch (...) { return c.Fail(); }
	};
	env.fileSize = [](void* ctx, const char* path, int64_t* size) noexcept -> int
	{
		Context& c = *static_cast<Context*>(ctx);
		try { *size = FileSystem::FileSize(path); return 0; } catch (...) { return c.Fail(); }
	};
	env.read = [](void* ctx, const char* path, uint8_t* buf, size_t len) noexcept -> int64_t
	{
		Context& c = *static_cast<Context*>(ctx);
		try
		{
			DiskFile file;
			if (!file.Open(path, DiskFile::omRead))
			{
				return -2;
			}
			int readBytes = (int)file.Read(buf, len);
			if (readBytes != (int)len && file.Error())
			{
				return -3;
			}
			file.Close();
			return readBytes;
		}
		catch (...) { return c.Fail(); }
	};
	env.loadPar = [](void* ctx, const char* path, size_t* count) noexcept -> int
	{
		Context& c = *static_cast<Context*>(ctx);
		try
		{
			c.repairer = std::make_unique<ParRenamerRepairer>();
			if (!c.repairer->LoadPacketsFromFile(path) || FileSystem::FileSize(path) == 0)
			{
				c.repairer.reset();
				return 0;
			}
			*count = c.repairer->sourcefilemap.size();
			c.entry = c.repairer->sourcefilemap.begin();
			return 1;
		}
		catch (...) { return c.Fail(); }
	};
	env.parEntry = [](void* ctx, void* sink,
		void (*add)(void*, int, const char*, const char*)) noexcept -> int
	{
		Context& c = *static_cast<Context*>(ctx);
		try
		{
			Par2::Par2RepairerSourceFile* sourceFile = (c.entry++)->second;
			if (!sourceFile || !sourceFile->GetDescriptionPacket())
			{
				add(sink, 1, nullptr, nullptr);
				return 0;
			}
			std::string filename = Par2::DescriptionPacket::TranslateFilenameFromPar2ToLocal(
				c.self->m_nout, c.self->m_nout, Par2::nlNormal, sourceFile->GetDescriptionPacket()->FileName());
			std::string hash = sourceFile->GetDescriptionPacket()->Hash16k().print();
			add(sink, 0, filename.c_str(), hash.c_str());
			return 0;
		}
		catch (...) { return c.Fail(); }
	};
	env.closePar = [](void* ctx) noexcept
	{
		static_cast<Context*>(ctx)->repairer.reset();
	};
	env.findMainPars = [](void* ctx, const char* dir, void* sink, void (*add)(void*, const char*)) noexcept -> int
	{
		Context& c = *static_cast<Context*>(ctx);
		try
		{
			ParParser::ParFileList parFileList;
			ParParser::FindMainPars(dir, &parFileList);
			for (std::string& parFilename : parFileList)
			{
				add(sink, parFilename.c_str());
			}
			return 0;
		}
		catch (...) { return c.Fail(); }
	};
	env.registerParred = [](void* ctx, const char* name) noexcept -> int
	{
		Context& c = *static_cast<Context*>(ctx);
		try { c.self->RegisterParredFile(name); return 0; } catch (...) { return c.Fail(); }
	};
	env.rename = [](void* ctx, const char* src, const char* dst) noexcept -> int
	{
		// RenameFile counts in m_renamedCount, the count Rust keeps: the
		// rename is reported, the member restored (a rename counted before
		// an exception is added back after the run)
		Context& c = *static_cast<Context*>(ctx);
		int renamedCount = c.self->m_renamedCount;
		try
		{
			c.self->RenameFile(src, dst);
			bool renamed = c.self->m_renamedCount != renamedCount;
			c.self->m_renamedCount = renamedCount;
			return renamed ? 1 : 0;
		}
		catch (...)
		{
			c.renamedOnError = c.self->m_renamedCount - renamedCount;
			c.self->m_renamedCount = renamedCount;
			return c.Fail();
		}
	};
	env.ignoredExt = [](void* ctx, const char* name) noexcept -> int
	{
		Context& c = *static_cast<Context*>(ctx);
		try { return Util::MatchFileExt(name, g_Options->GetParIgnoreExt(), ",;") ? 1 : 0; } catch (...) { return c.Fail(); }
	};
	env.fold = ParParser::FilenameFold;
	env.stateChanged = [](void* ctx, NzbgetRsParRenamerState* state) noexcept
	{
		ParRenamer* self = static_cast<Context*>(ctx)->self;
		self->m_fileCount = state->fileCount;
		self->m_curFile = state->curFile;
		self->m_renamedCount = state->renamedCount;
		self->m_hasMissedFiles = state->hasMissedFiles != 0;
		self->m_hasDamagedParFiles = state->hasDamagedParFiles != 0;
		state->detectMissing = self->m_detectMissing;
	};
	env.previousDirs = [](void* ctx, const char*, void* sink, void (*add)(void*, const char*)) noexcept -> int
	{
		Context& c = *static_cast<Context*>(ctx);
		try
		{
			for (CString& dir : c.self->m_dirList) { add(sink, dir); }
			return 0;
		}
		catch (...) { return c.Fail(); }
	};
	env.rememberDir = [](void* ctx, const char* dir) noexcept -> int
	{
		Context& c = *static_cast<Context*>(ctx);
		try { c.self->m_dirList.emplace_back(dir); return 0; } catch (...) { return c.Fail(); }
	};
	env.destination = [](void* ctx, const char*, void* sink, void (*add)(void*, const char*)) noexcept -> int
	{
		Context& c = *static_cast<Context*>(ctx);
		try { add(sink, c.self->m_destDir); return 0; } catch (...) { return c.Fail(); }
	};

	NzbgetRsParRenamerState state{m_fileCount, m_curFile, m_renamedCount, m_hasMissedFiles, m_hasDamagedParFiles, m_detectMissing};
	nzbget_rs_par_renamer_execute(&env, &state, m_infoName, m_destDir, PATH_SEPARATOR);
	m_fileCount = state.fileCount;
	m_curFile = state.curFile;
	m_renamedCount = state.renamedCount + context.renamedOnError;
	m_hasMissedFiles = state.hasMissedFiles != 0;
	m_hasDamagedParFiles = state.hasDamagedParFiles != 0;

	if (context.error)
	{
		std::rethrow_exception(context.error);
	}
}

bool ParRenamer::ListParredFiles(const char* parFilename, std::vector<std::string>& filenames)
{
	// a main par2 index is small; anything larger is a recovery volume whose
	// packets would take long to scan
	if (FileSystem::FileSize(parFilename) > MaxIndexParSize)
	{
		return false;
	}

	ParRenamerRepairer repairer;
	if (!repairer.LoadPacketsFromFile(parFilename))
	{
		return false;
	}

	for (std::pair<const Par2::MD5Hash, Par2::Par2RepairerSourceFile*>& entry : repairer.sourcefilemap)
	{
		Par2::Par2RepairerSourceFile* sourceFile = entry.second;
		if (sourceFile && sourceFile->GetDescriptionPacket())
		{
			std::string filename = Par2::DescriptionPacket::TranslateFilenameFromPar2ToLocal(
				repairer.m_nout, repairer.m_nout, Par2::nlNormal,
				sourceFile->GetDescriptionPacket()->FileName());
			filenames.emplace_back(FileSystem::BaseFileName(filename.c_str()));
		}
	}
	return true;
}

void ParRenamer::RenameFile(const char* srcFilename, const char* destFileName)
{
	const auto [oldPath, oldName] = FileSystem::SplitPathAndFilename(srcFilename);
	const auto [newPath, newName] = FileSystem::SplitPathAndFilename(destFileName);

	PrintMessage(Message::mkInfo, "Renaming %s to %s", oldName.c_str(), newName.c_str());

	if (oldPath != newPath)
	{
		CString errmsg;
		if (!FileSystem::ForceDirectories(newPath.c_str(), errmsg))
		{
			PrintMessage(Message::mkError, 
				"Failed to rename file %s to %s. Could not create the target directory \"%s\"",
				oldName.c_str(), newName.c_str(), newPath.c_str()
			);

			return;
		}
	}

	if (!FileSystem::MoveFile(srcFilename, destFileName))
	{
		PrintMessage(Message::mkError, 
			"Failed to rename file %s to %s: %s", 
			srcFilename, destFileName, *FileSystem::GetLastErrorMessage()
		);
		return;
	}

	++m_renamedCount;

	// notify about new file name
	std::string oldRelName = MakeRelativeName(srcFilename);
	std::string newRelName = MakeRelativeName(destFileName);
	RegisterRenamedFile(oldRelName.c_str(), newRelName.c_str());
}

std::string ParRenamer::MakeRelativeName(const char* fullFilename)
{
	fs::path canonicalDest = fs::weakly_canonical(fs::u8path(m_destDir.Str()));
	fs::path canonicalFile = fs::weakly_canonical(fs::u8path(fullFilename));
	fs::path rel = canonicalFile.lexically_relative(canonicalDest);

	if (rel.empty() || *rel.begin() == "..")
	{
		return fs::u8string(canonicalFile.filename());
	}

	return fs::u8string(rel);
}

#endif
