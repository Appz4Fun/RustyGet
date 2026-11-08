/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2007-2017 Andrey Prygunkov <hugbug@users.sourceforge.net>
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


#ifndef FILESYSTEM_H
#define FILESYSTEM_H

#include <optional>
#include <chrono>
#include "NString.h"
#include <filesystem>

namespace fs
{
using namespace std::filesystem;
using error_code = std::error_code;
using errc = std::errc;

inline fs::path u8path(std::string_view pathStr)
{
	return fs::path(pathStr);
}

inline std::string u8string(const path& p)
{
	return p.string();
}
}



namespace fs
{
inline path make_unique_filename()
{
	return std::to_string(std::chrono::steady_clock::now().time_since_epoch().count());
}

inline fs::path make_unique_filename(const fs::path& targetPath)
{
	fs::error_code ec;
	if (!fs::exists(targetPath, ec) && !ec)
	{
		return targetPath;
	}

	const fs::path baseDir = targetPath.parent_path();
	const std::string stem = fs::u8string(targetPath.stem());
	const std::string ext = fs::u8string(targetPath.extension());

	int counter = 1;
	fs::path uniquePath;
	do
	{
		uniquePath = baseDir / u8path(stem + " (" + std::to_string(counter++) + ")" + ext);
	} while (fs::exists(uniquePath, ec) && !ec);

	return uniquePath;
}

void move_file(const fs::path& src, const fs::path& dest, fs::error_code& ec) noexcept;

inline void move_file(const fs::path& src, const fs::path& dest)
{
	error_code ec;
	move_file(src, dest, ec);
	if (ec) throw std::runtime_error(ec.message());
}
}

class FileSystem
{
	struct DiskState
	{
		int64 available;
		int64 total;
	};

public:
	static CString GetLastErrorMessage();
	static char* BaseFileName(const char* filename);
	static std::pair<std::string, std::string> SplitPathAndFilename(const std::string& fullPath);
	static bool SameFilename(const char* filename1, const char* filename2);
	static void NormalizePathSeparators(char* path);
	static std::optional<std::string> GetRealPath(const std::string& path);
	static bool LoadFileIntoBuffer(const char* filename, CharBuffer& buffer, bool addTrailingNull);
	static bool SaveBufferIntoFile(const char* filename, const char* buffer, int bufLen);
	static bool AllocateFile(const char* filename, int64 size, bool sparse, CString& errmsg);
	static bool TruncateFile(const char* filename, int size);
	static CString MakeValidFilename(const char* filename, bool allowSlashes = false);
	static std::string SanitizePathSegment(std::string_view name);
	static std::string SanitizeRelativePath(std::string_view path);
	static bool ReservedChar(char ch);
	static CString MakeUniqueFilename(const char* destDir, const char* basename);
	static bool MoveFile(const char* srcFilename, const char* dstFilename);
	static bool CopyFile(const char* srcFilename, const char* dstFilename);
	static bool DeleteFile(const char* filename);
	static bool FileExists(const char* filename);
	static bool DirectoryExists(const char* dirFilename);
	// a symbolic link (or a reparse point on Windows), not followed
	static bool IsLink(const char* filename);
	static bool CreateDirectory(const char* dirFilename);
	static bool CreateDirectoryExclusive(const char* dirFilename);
	static std::string ExtractFilePathFromCmd(const std::string& path);
	static std::string EscapePathForShell(const std::string& path);
	static std::optional<std::string> GetFileExtension(std::string_view filename);

	/* Delete empty directory */
	static bool RemoveDirectory(const char* dirFilename);

	/* Delete directory which is empty or contains only hidden files or directories */
	static bool DeleteDirectory(const char* dirFilename);

	static bool DeleteDirectoryWithContent(const char* dirFilename, CString& errmsg);
	static bool ForceDirectories(const char* path, CString& errmsg);
	static bool CreateHardLink(const char *from, const char *to, CString& errmsg);
	static CString GetCurrentDirectory();
	static bool SetCurrentDirectory(const char* dirFilename);
	static int64 FileSize(const char* filename);
	static std::optional<DiskState> GetDiskState(const char* path);
	static bool DirEmpty(const char* dirFilename);
	static bool RenameBak(const char* filename, const char* bakPart, bool removeOldExtension, CString& newName);
	static CString ExpandHomePath(const char* filename);
	static void FixExecPermission(const char* filename);
	static bool RestoreFileOrDirPermissions(const char* filename);
	static bool RestoreFilePermissions(const char* filename);
	static bool RestoreDirPermissions(const char* filename);
	static bool RestorePermissions(const char* filename, mode_t mode);
	static mode_t uMask;
	static CString ExpandFileName(const char* filename);
	static CString GetExeFileName(const char* argv0);

	/* Flush disk buffers for file with given descriptor */
	static bool FlushFileBuffers(int fileDescriptor, CString& errmsg);

	/* Flush disk buffers for file metadata (after file renaming) */
	static bool FlushDirBuffers(const char* filename, CString& errmsg);

	static CString MakeExtendedPath(const char* path, bool force);
};

class DirBrowser
{
public:
	DirBrowser(const char* path, bool snapshot = true);
	~DirBrowser();
	const char* Next();

private:
	DIR* m_dir = nullptr;
	struct dirent* m_findData;

	bool m_snapshot;
	typedef std::deque<CString> FileList;
	FileList m_snapshotFiles;
	FileList::iterator m_snapshotIter;

	const char* InternNext();
};

class DiskFile
{
public:
	enum EOpenMode
	{
		omRead, // file must exist
		omReadWrite, // file must exist
		omWrite, // create new or overwrite existing
		omAppend // create new or append to existing
	};

	enum ESeekOrigin
	{
		soSet,
		soCur,
		soEnd
	};

	DiskFile() = default;
	DiskFile(const DiskFile&) = delete;
	virtual ~DiskFile();
	bool Open(const char* filename, EOpenMode mode);
	bool Close();
	bool Active() { return m_file != nullptr; }
	int64 Read(void* buffer, int64 size);
	int64 Write(const void* buffer, int64 size);
	int64 Position();
	bool Seek(int64 position, ESeekOrigin origin = soSet);
	bool Eof();
	bool Error();
	int64 Print(const char* format, ...) PRINTF_SYNTAX(2);
	virtual char* ReadLine(char* buffer, int64 size);
	bool SetWriteBuffer(int size);
	bool Flush();
	bool Sync(CString& errmsg);
	int GetFileDescriptor() const { return m_file ? fileno(m_file) : -1; }

private:
	FILE* m_file = nullptr;
};

#include "nzbget_rs.h"
#include <exception>
#include <memory>

// Keep the Rust allocation owned even if std::string's allocation throws.
static inline std::string CopyRustPathText(NzbgetRsBuf buf)
{
	const auto release = [buf](char*) { nzbget_rs_free(buf); };
	const std::unique_ptr<char, decltype(release)> owner(buf.data, release);
	return std::string(buf.data, buf.len);
}
#include "Util.h"
#include "Log.h"

// Keep the platform's stat ABI and errno behavior (Rust metadata can use
// statx, whose availability probe changes errno, and different time widths).
static inline int RustStat(const char* path)
{
	struct stat buffer;
	if (stat(path, &buffer) != 0)
	{
		return -errno;
	}
	return S_ISREG(buffer.st_mode) ? 1 : S_ISDIR(buffer.st_mode) ? 2 : 0;
}

static inline int RustStatvfs(const char* path, uint64_t* bavail, uint64_t* blocks, uint64_t* frsize)
{
	struct statvfs diskdata;
	if (statvfs(path, &diskdata) != 0)
	{
		return 0;
	}
	*bavail = static_cast<uint64_t>(diskdata.f_bavail);
	*blocks = static_cast<uint64_t>(diskdata.f_blocks);
	*frsize = static_cast<uint64_t>(diskdata.f_frsize);
	return 1;
}

inline CString FileSystem::GetLastErrorMessage()
{
	// rust/src/fsops.rs: the XSI strerror_r (glibc's GNU strerror_r left the
	// buffer empty for every known error, so the messages had no reason)
	BString<1024> msg;
	nzbget_rs_error_message(errno, msg);
	return *msg;
}

inline void FileSystem::NormalizePathSeparators(char* path)
{
	// rust/src/paths.rs
	nzbget_rs_normalize_path_separators(path);
}

inline std::optional<std::string> FileSystem::GetRealPath(const std::string& path)
{
	if (path.empty()) return std::nullopt;

	if (char* realPath = realpath(path.c_str(), nullptr))
	{
		std::string res = realPath;
		free(realPath);
		return res;
	}

	return std::nullopt;
}

inline bool FileSystem::ForceDirectories(const char* path, CString& errmsg)
{
	// rust/src/fsops.rs
	NzbgetRsBuf msg{};
	bool ok = nzbget_rs_force_directories(path, &msg, RustStat) != 0;
	errmsg.Clear();
	if (!ok)
	{
		errmsg = msg.data;
	}
	nzbget_rs_free(msg);
	return ok;
}

inline bool FileSystem::CreateHardLink(const char *from, const char *to, CString& errmsg)
{
	const auto [newPath, _] = SplitPathAndFilename(to);
	if (!ForceDirectories(newPath.c_str(), errmsg))
	{
		return false;
	}

	if (link(from, to) != 0)
	{
		errmsg = GetLastErrorMessage();
		return false;
	}

	return true;
}

inline CString FileSystem::GetCurrentDirectory()
{
	char str[1024];
	getcwd(str, 1024);
	return str;
}

inline bool FileSystem::SetCurrentDirectory(const char* dirFilename)
{
	return chdir(dirFilename) == 0;
}

inline bool FileSystem::DirEmpty(const char* dirFilename)
{
	DirBrowser dir(dirFilename);
	return dir.Next() == nullptr;
}

inline bool FileSystem::LoadFileIntoBuffer(const char* filename, CharBuffer& buffer, bool addTrailingNull)
{
	// a regular file only: a directory opens too, and its "size" (-1, or a block
	// size it can't be read for) gave a null buffer written through, or unread
	// bytes passed on as its content
	if (!FileExists(filename))
	{
		return false;
	}

	DiskFile file;
	if (!file.Open(filename, DiskFile::omRead))
	{
		return false;
	}

	// obtain file size.
	file.Seek(0, DiskFile::soEnd);
	int64 fileSize = file.Position();
	file.Seek(0);
	int size = 0;
	int bufSize = 0;
	if (nzbget_rs_ufs_fs_validate_load_file_size(
		fileSize, addTrailingNull ? 1 : 0, &size, &bufSize) == 0)
	{
		return false;
	}
	buffer.Reserve(bufSize);
	if (bufSize > 0 && !buffer)
	{
		return false;
	}

	// copy the file into the buffer.
	if (size > 0 && file.Read(buffer, size) != size)
	{
		return false;
	}
	file.Close();

	if (addTrailingNull)
	{
		buffer[size] = 0;
	}

	return true;
}

inline bool FileSystem::SaveBufferIntoFile(const char* filename, const char* buffer, int bufLen)
{
	return nzbget_rs_fs_save_buffer_into_file(
		filename, buffer, bufLen > 0 ? static_cast<size_t>(bufLen) : 0) != 0;
}

inline bool FileSystem::AllocateFile(const char* filename, int64 size, [[maybe_unused]] bool sparse, CString& errmsg)
{
	NzbgetRsBuf errBuf{};
	bool ok = nzbget_rs_fs_allocate_file(filename, size, &errBuf) != 0;
	if (errBuf.len == 0)
	{
		errmsg.Clear();
	}
	else
	{
		errmsg.Set(errBuf.data ? errBuf.data : "", (int)errBuf.len);
	}
	nzbget_rs_free(errBuf);
	return ok;
}

inline bool FileSystem::TruncateFile(const char* filename, int size)
{
	return truncate(filename, size) == 0;
}

inline char* FileSystem::BaseFileName(const char* filename)
{
	// rust/src/paths.rs
	return const_cast<char*>(filename) + nzbget_rs_path_position(0, filename, strlen(filename));
}

inline std::pair<std::string, std::string> FileSystem::SplitPathAndFilename(const std::string& fullPath)
{
	// rust/src/paths.rs
	size_t slash = nzbget_rs_path_position(1, fullPath.data(), fullPath.size());
	if (slash == SIZE_MAX)
	{
		return std::make_pair(fullPath, "");
	}
	return std::make_pair(fullPath.substr(0, slash), fullPath.substr(slash + 1));
}

inline bool FileSystem::ReservedChar(char ch)
{
	// rust/src/paths.rs
	return nzbget_rs_reserved_char(ch) != 0;
}

//replace bad chars in filename
inline CString FileSystem::MakeValidFilename(const char* filename, bool allowSlashes)
{
	// rust/src/paths.rs
	NzbgetRsBuf buf = nzbget_rs_path_text(0, filename, strlen(filename), allowSlashes);
	CString result(buf.data, static_cast<int>(buf.len));
	nzbget_rs_free(buf);
	return result;
}

inline std::string FileSystem::SanitizePathSegment(std::string_view name)
{
	// rust/src/paths.rs
	return CopyRustPathText(nzbget_rs_path_text(1, name.data(), name.size(), 0));
}

inline std::string FileSystem::SanitizeRelativePath(std::string_view path)
{
	// rust/src/paths.rs
	return CopyRustPathText(nzbget_rs_path_text(2, path.data(), path.size(), 0));
}

inline CString FileSystem::MakeUniqueFilename(const char* destDir, const char* basename)
{
	// rust/src/fsops.rs
	NzbgetRsBuf buf = nzbget_rs_make_unique_filename(destDir, basename, RustStat);
	CString result(buf.data, static_cast<int>(buf.len));
	nzbget_rs_free(buf);
	return result;
}

inline void fs::move_file(const fs::path& src, const fs::path& dest, fs::error_code& ec) noexcept
{
	int err = nzbget_rs_fs_move_file_no_overwrite(src.c_str(), dest.c_str());
	if (err == 0)
	{
		ec.clear();
	}
	else
	{
		ec = std::make_error_code(static_cast<std::errc>(err));
	}
}

inline bool FileSystem::MoveFile(const char* srcFilename, const char* dstFilename)
{
	return nzbget_rs_fs_move_file(srcFilename, dstFilename) != 0;
}

inline bool FileSystem::CopyFile(const char* srcFilename, const char* dstFilename)
{
	return nzbget_rs_fs_copy_file(srcFilename, dstFilename) != 0;
}

inline bool FileSystem::DeleteFile(const char* filename)
{
	return nzbget_rs_fs_delete_file(filename) != 0;
}

inline bool FileSystem::FileExists(const char* filename)
{
	// rust/src/fsops.rs (stat, a regular file)
	return nzbget_rs_file_exists(filename, RustStat) != 0;
}

inline bool FileSystem::DirectoryExists(const char* dirFilename)
{
	// rust/src/fsops.rs (stat, a directory)
	return nzbget_rs_directory_exists(dirFilename, RustStat) != 0;
}

inline bool FileSystem::CreateDirectory(const char* dirFilename)
{
	nzbget_rs_fs_create_directory_exclusive(dirFilename);
	return DirectoryExists(dirFilename);
}

inline bool FileSystem::CreateDirectoryExclusive(const char* dirFilename)
{
	return nzbget_rs_fs_create_directory_exclusive(dirFilename) != 0;
}

inline bool FileSystem::RemoveDirectory(const char* dirFilename)
{
	return nzbget_rs_fs_remove_directory(dirFilename) != 0;
}


inline std::string FileSystem::ExtractFilePathFromCmd(const std::string& path)
{
	// rust/src/paths.rs
	return path.substr(0, nzbget_rs_path_position(2, path.data(), path.size()));
}

inline std::string FileSystem::EscapePathForShell(const std::string& path)
{
	// rust/src/paths.rs
	return CopyRustPathText(nzbget_rs_path_text(3, path.data(), path.size(), 0));
}

inline std::optional<std::string> FileSystem::GetFileExtension(std::string_view filename)
{
	// rust/src/fsdelete.rs
	ptrdiff_t extIdx = nzbget_rs_file_extension(filename.data(), filename.size());
	if (extIdx < 0)
		return std::nullopt;

	return std::string(filename.substr(extIdx));
}

namespace
{
enum class PathType
{
	Error,
	File,
	Directory,
	Link,
	DirectoryLink
};

PathType GetPathTypeNoFollow(const char* filename)
{
	struct stat buffer;
	if (lstat(filename, &buffer) != 0)
	{
		return PathType::Error;
	}
	if (S_ISLNK(buffer.st_mode))
	{
		return PathType::Link;
	}
	return S_ISDIR(buffer.st_mode) ? PathType::Directory : PathType::File;
}

bool DeleteLink(const char* filename, PathType pathType)
{
	(void)pathType;
	return FileSystem::DeleteFile(filename);
}

// The calls rust/src/fsdelete.rs makes. errno is kept between the calls as
// the C++ kept it (Rust's own allocations don't touch it) and restored when
// the operation returns; an exception ends the operation and is rethrown.
class RsFsOps
{
public:
	NzbgetRsFsOps ops{};

	RsFsOps() : m_errno(errno)
	{
		ops.ctx = this;
		ops.pathType = [](void* ctx, const char* path) noexcept -> int
		{
			return Call(ctx, [&] { return static_cast<int>(GetPathTypeNoFollow(path)); });
		};
		ops.removeDir = [](void* ctx, const char* path) noexcept -> int
		{
			return Call(ctx, [&] { return FileSystem::RemoveDirectory(path) ? 1 : 0; });
		};
		ops.deleteFile = [](void* ctx, const char* path) noexcept -> int
		{
			return Call(ctx, [&] { return FileSystem::DeleteFile(path) ? 1 : 0; });
		};
		ops.deleteLink = [](void* ctx, const char* path, int type) noexcept -> int
		{
			return Call(ctx, [&] { return DeleteLink(path, type == 4 ? PathType::DirectoryLink : PathType::Link) ? 1 : 0; });
		};
		ops.list = [](void* ctx, const char* dir, void* sink, void (*add)(void*, const char*)) noexcept -> int
		{
			return Call(ctx, [&]
			{
				DirBrowser browser(dir);
				while (const char* filename = browser.Next())
				{
					add(sink, filename);
				}
				return 0;
			});
		};
		ops.fileExists = [](void* ctx, const char* path) noexcept -> int
		{
			return Call(ctx, [&] { return FileSystem::FileExists(path) ? 1 : 0; });
		};
		ops.dirExists = [](void* ctx, const char* path) noexcept -> int
		{
			return Call(ctx, [&] { return FileSystem::DirectoryExists(path) ? 1 : 0; });
		};
		ops.moveFile = [](void* ctx, const char* from, const char* to) noexcept -> int
		{
			return Call(ctx, [&] { return FileSystem::MoveFile(from, to) ? 1 : 0; });
		};
		ops.lastError = [](void* ctx, void* sink, void (*set)(void*, const char*)) noexcept -> int
		{
			return Call(ctx, [&] { set(sink, FileSystem::GetLastErrorMessage()); return 0; });
		};
		ops.tooLong = [](void* ctx) noexcept -> int
		{
			return Call(ctx, [&] { errno = ENAMETOOLONG; return 0; });
		};
	}

	int Finish(int result)
	{
		errno = m_errno;
		if (m_error)
		{
			std::rethrow_exception(m_error);
		}
		return result;
	}

private:
	int m_errno;
	std::exception_ptr m_error;

	template <class F>
	static int Call(void* ctx, F f) noexcept
	{
		RsFsOps& self = *static_cast<RsFsOps*>(ctx);
		errno = self.m_errno;
		try
		{
			int result = f();
			self.m_errno = errno;
			return result;
		}
		catch (...)
		{
			self.m_errno = errno;
			self.m_error = std::current_exception();
			return -1;
		}
	}
};
}

inline bool FileSystem::IsLink(const char* filename)
{
	PathType type = GetPathTypeNoFollow(filename);
	return type == PathType::Link || type == PathType::DirectoryLink;
}

/* Delete directory which is empty or contains only hidden files or directories (whose names start with dot) */
inline bool FileSystem::DeleteDirectory(const char* dirFilename)
{
	if (!dirFilename)
	{
		errno = EFAULT;
		return false;
	}
	// rust/src/fsdelete.rs
	RsFsOps ops;
	return ops.Finish(nzbget_rs_delete_directory(&ops.ops, dirFilename, PATH_SEPARATOR)) == 1;
}

inline bool FileSystem::DeleteDirectoryWithContent(const char* dirFilename, CString& errmsg)
{
	// Keep the POSIX NULL-path error distinct from an empty path, without
	// passing NULL to lstat or printf (their declarations require non-NULL).
	if (!dirFilename)
	{
		errmsg.Clear();
		errno = EFAULT;
		NzbgetRsBuf errBuf = nzbget_rs_dqh_fs_format_null_delete_errmsg(*GetLastErrorMessage());
		errmsg.Set(errBuf.data ? errBuf.data : "", (int)errBuf.len);
		nzbget_rs_free(errBuf);
		return false;
	}
	// rust/src/fsdelete.rs
	RsFsOps ops;
	NzbgetRsBuf text{};
	int result = nzbget_rs_delete_directory_with_content(&ops.ops, dirFilename, PATH_SEPARATOR, &text);
	if (text.len == 0)
	{
		errmsg.Clear();
	}
	else
	{
		errmsg.Set(text.data, (int)text.len);
	}
	nzbget_rs_free(text);
	return ops.Finish(result) == 1;
}

inline int64 FileSystem::FileSize(const char* filename)
{
	return nzbget_rs_fs_file_size(filename);
}

inline std::optional<FileSystem::DiskState> FileSystem::GetDiskState(const char* path)
{
	int64 available = 0;
	int64 total = 0;
	if (nzbget_rs_fs_get_disk_state(path, RustStatvfs, &available, &total) != 0)
	{
		return FileSystem::DiskState{ available, total };
	}
	return std::nullopt;
}

inline bool FileSystem::RenameBak(const char* filename, const char* bakPart, bool removeOldExtension, CString& newName)
{
	if (!filename || !bakPart)
	{
		return false;
	}
	// rust/src/fsdelete.rs
	RsFsOps ops;
	NzbgetRsBuf name{};
	int result = nzbget_rs_rename_bak(&ops.ops, filename, bakPart, removeOldExtension, &name);
	newName.Set(name.data, (int)name.len);
	nzbget_rs_free(name);
	return ops.Finish(result) == 1;
}

inline CString FileSystem::ExpandHomePath(const char* filename)
{
	// rust/src/fsops.rs; the home directory looked up as before
	const char* home = nullptr;
	if (filename && filename[0] == '~' && filename[1] == '/')
	{
		home = getenv("HOME");
		if (!home)
		{
			struct passwd* pw = getpwuid(getuid());
			if (pw)
			{
				home = pw->pw_dir;
			}
		}
	}
	NzbgetRsBuf buf = nzbget_rs_expand_home_path(filename, home);
	CString result(buf.data, static_cast<int>(buf.len));
	nzbget_rs_free(buf);
	return result;
}

inline CString FileSystem::ExpandFileName(const char* filename)
{
	NzbgetRsBuf expBuf = nzbget_rs_fs_expand_file_name(filename);
	CString result(expBuf.data ? expBuf.data : "", static_cast<int>(expBuf.len));
	nzbget_rs_free(expBuf);
	return result;
}

inline CString FileSystem::GetExeFileName(const char* argv0)
{
	NzbgetRsBuf exeBuf = nzbget_rs_fs_get_exe_file_name(argv0);
	CString result(exeBuf.data ? exeBuf.data : "", static_cast<int>(exeBuf.len));
	nzbget_rs_free(exeBuf);
	return result;
}

inline bool FileSystem::SameFilename(const char* filename1, const char* filename2)
{
	return nzbget_rs_dqh_bytes_equal(filename1, filename2) != 0;
}

inline bool FileSystem::FlushFileBuffers(int fileDescriptor, CString& errmsg)
{
	NzbgetRsBuf errBuf{};
	bool ok = nzbget_rs_fs_flush_file_buffers(fileDescriptor, &errBuf) != 0;
	if (!ok)
	{
		errmsg.Set(errBuf.data ? errBuf.data : "", (int)errBuf.len);
	}
	nzbget_rs_free(errBuf);
	return ok;
}

inline bool FileSystem::FlushDirBuffers(const char* filename, CString& errmsg)
{
	NzbgetRsBuf errBuf{};
	bool ok = nzbget_rs_fs_flush_dir_buffers(filename, PATH_SEPARATOR, &errBuf) != 0;
	if (!ok)
	{
		errmsg.Set(errBuf.data ? errBuf.data : "", (int)errBuf.len);
	}
	nzbget_rs_free(errBuf);
	return ok;
}

inline mode_t FileSystem::uMask;

inline void FileSystem::FixExecPermission(const char* filename)
{
	nzbget_rs_fs_fix_exec_permission(filename);
}

inline bool FileSystem::RestoreFileOrDirPermissions(const char* filename)
{
	return nzbget_rs_fs_restore_file_or_dir_permissions(filename, static_cast<uint32_t>(FileSystem::uMask)) != 0;
}

inline bool FileSystem::RestoreFilePermissions(const char* filepath)
{
	return RestorePermissions(filepath, 0666);
}

inline bool FileSystem::RestoreDirPermissions(const char* filename)
{
	return RestorePermissions(filename, 0777);
}

inline bool FileSystem::RestorePermissions(const char* filename, mode_t mode)
{
	return nzbget_rs_fs_restore_permissions(
		filename, static_cast<uint32_t>(mode), static_cast<uint32_t>(FileSystem::uMask)) != 0;
}

inline CString FileSystem::MakeExtendedPath(const char* path, [[maybe_unused]] bool force)
{
	return path;
}


inline DirBrowser::DirBrowser(const char* path, bool snapshot) :
	m_snapshot(snapshot)
{
	if (m_snapshot)
	{
		DirBrowser dir(path, false);
		while (const char* filename = dir.Next())
		{
			m_snapshotFiles.emplace_back(filename);
		}
		m_snapshotIter = m_snapshotFiles.begin();
	}
	else
	{
		m_dir = opendir(path);
	}
}

inline DirBrowser::~DirBrowser()
{
	if (m_dir)
	{
		closedir(m_dir);
	}
}

inline const char* DirBrowser::InternNext()
{
	if (m_snapshot)
	{
		return m_snapshotIter == m_snapshotFiles.end() ? nullptr : **m_snapshotIter++;
	}
	else
	{
		if (m_dir)
		{
			m_findData = readdir(m_dir);
			if (m_findData)
			{
				return m_findData->d_name;
			}
		}
		return nullptr;
	}
}

inline const char* DirBrowser::Next()
{
	const char* filename = nullptr;
	for (filename = InternNext(); filename && nzbget_rs_ufs_dir_browser_is_dot_or_dotdot(filename) != 0; )
	{
		filename = InternNext();
	}
	return filename;
}


inline DiskFile::~DiskFile()
{
	if (m_file)
	{
		Close();
	}
}

inline bool DiskFile::Open(const char* filename, EOpenMode mode)
{
	const char* strmode = mode == omRead ? FOPEN_RB : mode == omReadWrite ?
		FOPEN_RBP : mode == omWrite ? FOPEN_WB : FOPEN_AB;
	int fd = nzbget_rs_fs_open_cloexec_fd(
		filename, (int)mode, (int)omRead, (int)omReadWrite, (int)omWrite);
	if (fd == -1)
	{
		return false;
	}
	m_file = fdopen(fd, strmode);
	if (!m_file)
	{
		int error = errno;
		close(fd);
		errno = error;
	}
	return m_file;
}

inline bool DiskFile::Close()
{
	if (m_file)
	{
		int ret = fclose(m_file);
		m_file = nullptr;
		return ret == 0;
	}
	else
	{
		return false;
	}
}

inline int64 DiskFile::Read(void* buffer, int64 size)
{
	return fread(buffer, 1, (size_t)size, m_file);
}

inline int64 DiskFile::Write(const void* buffer, int64 size)
{
	return fwrite(buffer, 1, (size_t)size, m_file);
}

inline int64 DiskFile::Print(const char* format, ...)
{
	va_list ap;
	va_start(ap, format);
	int ret = vfprintf(m_file, format, ap);
	va_end(ap);
	return ret;
}

inline char* DiskFile::ReadLine(char* buffer, int64 size)
{
	return fgets(buffer, (int)size, m_file);
}

inline int64 DiskFile::Position()
{
	return ftell(m_file);
}

inline bool DiskFile::Seek(int64 position, ESeekOrigin origin)
{
	return fseek(m_file, position,
		nzbget_rs_ufs_disk_file_select_seek_origin(
			(int)origin, (int)soCur, (int)soEnd, SEEK_SET, SEEK_CUR, SEEK_END)) == 0;
}

inline bool DiskFile::Eof()
{
	return feof(m_file) != 0;
}

inline bool DiskFile::Error()
{
	return ferror(m_file) != 0;
}

inline bool DiskFile::SetWriteBuffer(int size)
{
	return setvbuf(m_file, nullptr, _IOFBF, size) == 0;
}

inline bool DiskFile::Flush()
{
	return fflush(m_file) == 0;
}

inline bool DiskFile::Sync(CString& errmsg)
{
	return FileSystem::FlushFileBuffers(fileno(m_file), errmsg);
}

#endif
