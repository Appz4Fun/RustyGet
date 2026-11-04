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


#include "nzbget.h"
#include "FileSystem.h"
#include "nzbget_rs.h"
#include <exception>
#include <memory>

// Keep the Rust allocation owned even if std::string's allocation throws.
static std::string CopyRustPathText(NzbgetRsBuf buf)
{
	const auto release = [buf](char*) { nzbget_rs_free(buf); };
	const std::unique_ptr<char, decltype(release)> owner(buf.data, release);
	return std::string(buf.data, buf.len);
}
#include "Util.h"
#include "Log.h"

// Keep the platform's stat ABI and errno behavior (Rust metadata can use
// statx, whose availability probe changes errno, and different time widths).
static int RustStat(const char* path)
{
	struct stat buffer;
	if (stat(path, &buffer) != 0)
	{
		return -errno;
	}
	return S_ISREG(buffer.st_mode) ? 1 : S_ISDIR(buffer.st_mode) ? 2 : 0;
}

CString FileSystem::GetLastErrorMessage()
{
	// rust/src/fsops.rs: the XSI strerror_r (glibc's GNU strerror_r left the
	// buffer empty for every known error, so the messages had no reason)
	BString<1024> msg;
	nzbget_rs_error_message(errno, msg);
	return *msg;
}

void FileSystem::NormalizePathSeparators(char* path)
{
	// rust/src/paths.rs
	nzbget_rs_normalize_path_separators(path);
}

std::optional<std::string> FileSystem::GetRealPath(const std::string& path)
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

bool FileSystem::ForceDirectories(const char* path, CString& errmsg)
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

bool FileSystem::CreateHardLink(const char *from, const char *to, CString& errmsg)
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

CString FileSystem::GetCurrentDirectory()
{
	char str[1024];
	getcwd(str, 1024);
	return str;
}

bool FileSystem::SetCurrentDirectory(const char* dirFilename)
{
	return chdir(dirFilename) == 0;
}

bool FileSystem::DirEmpty(const char* dirFilename)
{
	DirBrowser dir(dirFilename);
	return dir.Next() == nullptr;
}

bool FileSystem::LoadFileIntoBuffer(const char* filename, CharBuffer& buffer, bool addTrailingNull)
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

bool FileSystem::SaveBufferIntoFile(const char* filename, const char* buffer, int bufLen)
{
	DiskFile file;
	if (!file.Open(filename, DiskFile::omWrite))
	{
		return false;
	}

	int writtenBytes = (int)file.Write(buffer, bufLen);
	file.Close();

	return writtenBytes == bufLen;
}

bool FileSystem::AllocateFile(const char* filename, int64 size, [[maybe_unused]] bool sparse, CString& errmsg)
{
	errmsg.Clear();
	bool ok = false;
	// create file; DiskFile opens it close-on-exec, so a script started by
	// another thread at this moment does not inherit the descriptor
	{
		DiskFile file;
		if (!file.Open(filename, DiskFile::omAppend))
		{
			errmsg = GetLastErrorMessage();
			return false;
		}
	}

	// there are no reliable function to expand file on POSIX, so we must try different approaches,
	// starting with the fastest one and hoping it will work
	// 1) set file size using function "truncate" (this is fast, if it works)
	truncate(filename, size);
	// check if it worked
	ok = FileSize(filename) == size;
	if (!ok)
	{
		// 2) truncate did not work, expanding the file by writing to it (that's slow)
		truncate(filename, 0);
		DiskFile file;
		if (!file.Open(filename, DiskFile::omAppend))
		{
			errmsg = GetLastErrorMessage();
			return false;
		}

		// write zeros in 16K chunks
		CharBuffer zeros(16 * 1024);
		memset(zeros, 0, zeros.Size());
		for (int64 remaining = size; remaining > 0;)
		{
			int64 needbytes = nzbget_rs_ufs_fs_calc_allocate_chunk_size(remaining, (int64)zeros.Size());
			int64 written = file.Write(zeros, needbytes);
			if (written != needbytes)
			{
				errmsg = GetLastErrorMessage();
				file.Close();
				return false;
			}
			remaining -= written;
		}
		file.Close();

		ok = FileSize(filename) == size;
		if (!ok)
		{
			errmsg = "created file has wrong size";
		}
	}
	return ok;
}

bool FileSystem::TruncateFile(const char* filename, int size)
{
	return truncate(filename, size) == 0;
}

char* FileSystem::BaseFileName(const char* filename)
{
	// rust/src/paths.rs
	return const_cast<char*>(filename) + nzbget_rs_path_position(0, filename, strlen(filename));
}

std::pair<std::string, std::string> FileSystem::SplitPathAndFilename(const std::string& fullPath)
{
	// rust/src/paths.rs
	size_t slash = nzbget_rs_path_position(1, fullPath.data(), fullPath.size());
	if (slash == SIZE_MAX)
	{
		return std::make_pair(fullPath, "");
	}
	return std::make_pair(fullPath.substr(0, slash), fullPath.substr(slash + 1));
}

bool FileSystem::ReservedChar(char ch)
{
	// rust/src/paths.rs
	return nzbget_rs_reserved_char(ch) != 0;
}

//replace bad chars in filename
CString FileSystem::MakeValidFilename(const char* filename, bool allowSlashes)
{
	// rust/src/paths.rs
	NzbgetRsBuf buf = nzbget_rs_path_text(0, filename, strlen(filename), allowSlashes);
	CString result(buf.data, static_cast<int>(buf.len));
	nzbget_rs_free(buf);
	return result;
}

std::string FileSystem::SanitizePathSegment(std::string_view name)
{
	// rust/src/paths.rs
	return CopyRustPathText(nzbget_rs_path_text(1, name.data(), name.size(), 0));
}

std::string FileSystem::SanitizeRelativePath(std::string_view path)
{
	// rust/src/paths.rs
	return CopyRustPathText(nzbget_rs_path_text(2, path.data(), path.size(), 0));
}

CString FileSystem::MakeUniqueFilename(const char* destDir, const char* basename)
{
	// rust/src/fsops.rs
	NzbgetRsBuf buf = nzbget_rs_make_unique_filename(destDir, basename, RustStat);
	CString result(buf.data, static_cast<int>(buf.len));
	nzbget_rs_free(buf);
	return result;
}

bool FileSystem::MoveFile(const char* srcFilename, const char* dstFilename)
{
	bool ok = rename(srcFilename, dstFilename) == 0;
	if (!ok && errno == EXDEV)
	{
		ok = CopyFile(srcFilename, dstFilename) && DeleteFile(srcFilename);
	}
	return ok;
}

bool FileSystem::CopyFile(const char* srcFilename, const char* dstFilename)
{
	DiskFile infile;
	if (!infile.Open(srcFilename, DiskFile::omRead))
	{
		return false;
	}

	DiskFile outfile;
	if (!outfile.Open(dstFilename, DiskFile::omWrite))
	{
		return false;
	}

	CharBuffer buffer(1024 * 50);

	// a failed write (disk full) or read must fail the copy: MoveFile deletes
	// the source after it
	bool ok = true;
	int64 copied = 0;
	while (true)
	{
		int64 cnt = infile.Read(buffer, buffer.Size());
		if (cnt <= 0)
		{
			break;
		}
		if (outfile.Write(buffer, cnt) != cnt)
		{
			ok = false;
			break;
		}
		copied += cnt;
	}

	infile.Close();
	ok = outfile.Close() && ok && copied == FileSize(srcFilename);

	if (!ok)
	{
		DeleteFile(dstFilename);
	}

	return ok;
}

bool FileSystem::DeleteFile(const char* filename)
{
	return remove(filename) == 0;
}

bool FileSystem::FileExists(const char* filename)
{
	// rust/src/fsops.rs (stat, a regular file)
	return nzbget_rs_file_exists(filename, RustStat) != 0;
}

bool FileSystem::DirectoryExists(const char* dirFilename)
{
	// rust/src/fsops.rs (stat, a directory)
	return nzbget_rs_directory_exists(dirFilename, RustStat) != 0;
}

bool FileSystem::CreateDirectory(const char* dirFilename)
{
	mkdir(dirFilename, S_DIRMODE);
	return DirectoryExists(dirFilename);
}

bool FileSystem::CreateDirectoryExclusive(const char* dirFilename)
{
	return mkdir(dirFilename, S_DIRMODE) == 0;
}

bool FileSystem::RemoveDirectory(const char* dirFilename)
{
	return rmdir(dirFilename) == 0;
}


std::string FileSystem::ExtractFilePathFromCmd(const std::string& path)
{
	// rust/src/paths.rs
	return path.substr(0, nzbget_rs_path_position(2, path.data(), path.size()));
}

std::string FileSystem::EscapePathForShell(const std::string& path)
{
	// rust/src/paths.rs
	return CopyRustPathText(nzbget_rs_path_text(3, path.data(), path.size(), 0));
}

std::optional<std::string> FileSystem::GetFileExtension(std::string_view filename)
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

bool FileSystem::IsLink(const char* filename)
{
	PathType type = GetPathTypeNoFollow(filename);
	return type == PathType::Link || type == PathType::DirectoryLink;
}

/* Delete directory which is empty or contains only hidden files or directories (whose names start with dot) */
bool FileSystem::DeleteDirectory(const char* dirFilename)
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

bool FileSystem::DeleteDirectoryWithContent(const char* dirFilename, CString& errmsg)
{
	// Keep the POSIX NULL-path error distinct from an empty path, without
	// passing NULL to lstat or printf (their declarations require non-NULL).
	if (!dirFilename)
	{
		errmsg.Clear();
		errno = EFAULT;
		errmsg.Format("could not delete (null): %s", *GetLastErrorMessage());
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

int64 FileSystem::FileSize(const char* filename)
{
	struct stat buffer;
	// stat does not promise usable output on failure. A watchdog checking an
	// output file before it exists must not mistake that data for a huge file.
	return stat(filename, &buffer) == 0 ? buffer.st_size : -1;
}

std::optional<FileSystem::DiskState> FileSystem::GetDiskState(const char* path)
{
	struct statvfs diskdata;
	if (!statvfs(path, &diskdata))
	{
		int64 available = Util::SafeIntCast<uint64, int64>(diskdata.f_bavail * diskdata.f_frsize);
		int64 total = Util::SafeIntCast<uint64, int64>(diskdata.f_blocks * diskdata.f_frsize);
		return FileSystem::DiskState{ available, total };
	}
	return std::nullopt;
}

bool FileSystem::RenameBak(const char* filename, const char* bakPart, bool removeOldExtension, CString& newName)
{
	// The Rust ABI treats NULL strings as empty. Preserve the platform C
	// library's original formatting/syscall behavior for NULL arguments.
	if (filename && bakPart)
	{
		// rust/src/fsdelete.rs
		RsFsOps ops;
		NzbgetRsBuf name{};
		int result = nzbget_rs_rename_bak(&ops.ops, filename, bakPart, removeOldExtension, &name);
		newName.Set(name.data, (int)name.len);
		nzbget_rs_free(name);
		return ops.Finish(result) == 1;
	}
	BString<1024> changedFilename;

	if (removeOldExtension)
	{
		changedFilename = filename;
		char* extension = strrchr(changedFilename, '.');
		if (extension)
		{
			*extension = '\0';
		}
	}

	newName.Format("%s.%s", removeOldExtension ? *changedFilename : filename, bakPart);

	int i = 2;
	while (FileExists(newName) || DirectoryExists(newName))
	{
		newName.Format("%s.%i.%s", removeOldExtension ? *changedFilename : filename, i++, bakPart);
	}

	return MoveFile(filename, newName);
}

CString FileSystem::ExpandHomePath(const char* filename)
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

CString FileSystem::ExpandFileName(const char* filename)
{
	CString result;
	result.Reserve(1024 - 1);
	size_t skipDotSlash = 0;
	if (nzbget_rs_ufs_fs_classify_expand_file_name(filename, &skipDotSlash) != 0)
	{
		char curDir[MAX_PATH + 1];
		getcwd(curDir, sizeof(curDir) - 1); // 1 char reserved for adding backslash
		result.Format("%s/%s", curDir, filename + skipDotSlash);
	}
	else
	{
		result = filename;
	}
	return result;
}

CString FileSystem::GetExeFileName(const char* argv0)
{
	CString exename;
	exename.Reserve(1024 - 1);
	exename[1024 - 1] = '\0';

	// Linux
	int r = readlink("/proc/self/exe", exename, 1024 - 1);
	if (r > 0)
	{
		exename[r] = '\0';
		return exename;
	}
	// FreeBSD
	r = readlink("/proc/curproc/file", exename, 1024 - 1);
	if (r > 0)
	{
		exename[r] = '\0';
		return exename;
	}

	exename = ExpandFileName(argv0);

	return exename;
}

bool FileSystem::SameFilename(const char* filename1, const char* filename2)
{
	return strcmp(filename1, filename2) == 0;
}

bool FileSystem::FlushFileBuffers(int fileDescriptor, CString& errmsg)
{
#ifdef HAVE_FULLFSYNC
	int ret = fcntl(fileDescriptor, F_FULLFSYNC) == -1 ? 1 : 0;
#elif HAVE_FDATASYNC
	int ret = fdatasync(fileDescriptor);
#else
	int ret = fsync(fileDescriptor);
#endif
	if (ret != 0)
	{
		errmsg = GetLastErrorMessage();
	}
	return ret == 0;
}

bool FileSystem::FlushDirBuffers(const char* filename, CString& errmsg)
{
	BString<1024> parentPath = filename;
	char* p = (char*)strrchr(parentPath, PATH_SEPARATOR);
	if (p)
	{
		*p = '\0';
	}
	// the directory is opened close-on-exec through DiskFile: the descriptor
	// is short-lived, but a script forked meanwhile must not inherit it
	DiskFile dir;
	if (!dir.Open(parentPath, DiskFile::omRead))
	{
		errmsg = GetLastErrorMessage();
		return false;
	}
	bool ok = FlushFileBuffers(dir.GetFileDescriptor(), errmsg);
	dir.Close();
	return ok;
}

mode_t FileSystem::uMask;

void FileSystem::FixExecPermission(const char* filename)
{
	struct stat buffer;
	bool ok = !stat(filename, &buffer);
	if (ok)
	{
		buffer.st_mode = buffer.st_mode | S_IXUSR | S_IXGRP | S_IXOTH;
		chmod(filename, buffer.st_mode);
	}
}

bool FileSystem::RestoreFileOrDirPermissions(const char* filename) 
{
	struct stat buffer;
	int ec = stat(filename, &buffer);
	if (ec != 0) 
	{

#ifdef DEBUG
		debug("Failed to read information for %s. Errno: %i", filename, ec);
#endif

		return false;
	}

	if (S_ISDIR(buffer.st_mode)) 
	{
		return RestoreDirPermissions(filename);
	} 

	return RestoreFilePermissions(filename);
}

bool FileSystem::RestoreFilePermissions(const char* filepath)
{
	mode_t mode = S_IRUSR | S_IWUSR | S_IRGRP | S_IWGRP | S_IROTH | S_IWOTH; // 0666
	return RestorePermissions(filepath, mode);
}

bool FileSystem::RestoreDirPermissions(const char* filename)
{
	mode_t mode = S_IRWXU | S_IRWXG | S_IRWXO; // 0777
	return RestorePermissions(filename, mode);
}

bool FileSystem::RestorePermissions(const char* filename, mode_t mode) 
{
	mode_t permissions = mode & ~FileSystem::uMask;
	int ec = chmod(filename, permissions);
	if (ec == 0)
	{

#ifdef DEBUG
		debug("Permissions %o was set for %s", permissions, filename);
#endif

		return true;
	} 

#ifdef DEBUG
	debug("Failed to set permissions for %s. Errno %i", filename, ec);
#endif

	return false;
}

CString FileSystem::MakeExtendedPath(const char* path, [[maybe_unused]] bool force)
{
	return path;
}


DirBrowser::DirBrowser(const char* path, bool snapshot) :
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

DirBrowser::~DirBrowser()
{
	if (m_dir)
	{
		closedir(m_dir);
	}
}

const char* DirBrowser::InternNext()
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

const char* DirBrowser::Next()
{
	const char* filename = nullptr;
	for (filename = InternNext(); filename && nzbget_rs_ufs_dir_browser_is_dot_or_dotdot(filename) != 0; )
	{
		filename = InternNext();
	}
	return filename;
}


DiskFile::~DiskFile()
{
	if (m_file)
	{
		Close();
	}
}

bool DiskFile::Open(const char* filename, EOpenMode mode)
{
	const char* strmode = mode == omRead ? FOPEN_RB : mode == omReadWrite ?
		FOPEN_RBP : mode == omWrite ? FOPEN_WB : FOPEN_AB;
	// Close-on-exec from the start: the scripts nzbget starts don't inherit
	// the log, the queue state files and the articles being written. (The
	// "e" letter of fopen does that on glibc, while macOS ignores it.)
	int flags = mode == omRead ? O_RDONLY : mode == omReadWrite ? O_RDWR :
		mode == omWrite ? O_WRONLY | O_CREAT | O_TRUNC : O_WRONLY | O_CREAT | O_APPEND;
	int fd = open(filename, flags | O_CLOEXEC, 0666);
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

bool DiskFile::Close()
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

int64 DiskFile::Read(void* buffer, int64 size)
{
	return fread(buffer, 1, (size_t)size, m_file);
}

int64 DiskFile::Write(const void* buffer, int64 size)
{
	return fwrite(buffer, 1, (size_t)size, m_file);
}

int64 DiskFile::Print(const char* format, ...)
{
	va_list ap;
	va_start(ap, format);
	int ret = vfprintf(m_file, format, ap);
	va_end(ap);
	return ret;
}

char* DiskFile::ReadLine(char* buffer, int64 size)
{
	return fgets(buffer, (int)size, m_file);
}

int64 DiskFile::Position()
{
	return ftell(m_file);
}

bool DiskFile::Seek(int64 position, ESeekOrigin origin)
{
	return fseek(m_file, position,
		nzbget_rs_ufs_disk_file_select_seek_origin(
			(int)origin, (int)soCur, (int)soEnd, SEEK_SET, SEEK_CUR, SEEK_END)) == 0;
}

bool DiskFile::Eof()
{
	return feof(m_file) != 0;
}

bool DiskFile::Error()
{
	return ferror(m_file) != 0;
}

bool DiskFile::SetWriteBuffer(int size)
{
	return setvbuf(m_file, nullptr, _IOFBF, size) == 0;
}

bool DiskFile::Flush()
{
	return fflush(m_file) == 0;
}

bool DiskFile::Sync(CString& errmsg)
{
	return FileSystem::FlushFileBuffers(fileno(m_file), errmsg);
}
