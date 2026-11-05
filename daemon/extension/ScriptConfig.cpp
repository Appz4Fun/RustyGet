/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2013-2016 Andrey Prygunkov <hugbug@users.sourceforge.net>
 *  Copyright (C) 2023 Denis <denis@nzbget.com>
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

#include "Util.h"
#include "FileSystem.h"
#include "Options.h"
#include "Log.h"
#include "ScriptConfig.h"
#include "nzbget_rs.h"
#if defined(__APPLE__) || defined(__linux__)
#define NZBGET_CIPHER_ATOMIC_CONFIG
#include <sys/stat.h>
#include <fcntl.h>
#include <unistd.h>
#include <sys/xattr.h>
#ifdef __APPLE__
#include <copyfile.h>
#endif
#endif

namespace
{
#ifdef NZBGET_CIPHER_ATOMIC_CONFIG
struct CipherDescriptor
{
	int value = -1;
	explicit CipherDescriptor(int fd = -1) : value(fd) {}
	~CipherDescriptor() { if (value >= 0) close(value); }
	CipherDescriptor(const CipherDescriptor&) = delete;
	CipherDescriptor& operator=(const CipherDescriptor&) = delete;
};

bool SameCipherFile(const struct stat& a, const struct stat& b)
{
#ifdef __APPLE__
	return a.st_dev == b.st_dev && a.st_ino == b.st_ino && a.st_size == b.st_size &&
		a.st_mode == b.st_mode && a.st_uid == b.st_uid && a.st_gid == b.st_gid &&
		a.st_mtimespec.tv_sec == b.st_mtimespec.tv_sec && a.st_mtimespec.tv_nsec == b.st_mtimespec.tv_nsec &&
		a.st_ctimespec.tv_sec == b.st_ctimespec.tv_sec && a.st_ctimespec.tv_nsec == b.st_ctimespec.tv_nsec;
#else
	return a.st_dev == b.st_dev && a.st_ino == b.st_ino && a.st_size == b.st_size &&
		a.st_mode == b.st_mode && a.st_uid == b.st_uid && a.st_gid == b.st_gid &&
		a.st_mtim.tv_sec == b.st_mtim.tv_sec && a.st_mtim.tv_nsec == b.st_mtim.tv_nsec &&
		a.st_ctim.tv_sec == b.st_ctim.tv_sec && a.st_ctim.tv_nsec == b.st_ctim.tv_nsec;
#endif
}

bool ReadCipherDescriptor(int fd, std::string& output, struct stat& before)
{
	if (fstat(fd, &before) != 0 || !S_ISREG(before.st_mode) || before.st_size < 0 || before.st_size > 16 * 1024 * 1024)
	{
		return false;
	}
	output.resize(static_cast<size_t>(before.st_size));
	size_t used = 0;
	while (used < output.size())
	{
		ssize_t count = pread(fd, output.data() + used, output.size() - used, static_cast<off_t>(used));
		if (count < 0 && errno == EINTR) continue;
		if (count <= 0) return false;
		used += static_cast<size_t>(count);
	}
	char extra;
	if (pread(fd, &extra, 1, before.st_size) != 0) return false;
	struct stat after;
	return fstat(fd, &after) == 0 && SameCipherFile(before, after);
}

bool CopyCipherAttributes(int source, int target)
{
#ifdef __APPLE__
	return fcopyfile(source, target, nullptr, COPYFILE_SECURITY | COPYFILE_XATTR) == 0;
#else
	ssize_t size = flistxattr(source, nullptr, 0);
	if (size < 0)
	{
		int error = errno;
		return error == ENOTSUP || error == EOPNOTSUPP;
	}
	if (size > 65536) return false;
	std::vector<char> names(static_cast<size_t>(size));
	if (size && flistxattr(source, names.data(), names.size()) != size) return false;
	size_t cursor = 0, total = 0;
	while (cursor < names.size())
	{
		const char* name = names.data() + cursor;
		size_t length = strnlen(name, names.size() - cursor);
		if (length == names.size() - cursor) return false;
		ssize_t bytes = fgetxattr(source, name, nullptr, 0);
		if (bytes < 0 || static_cast<size_t>(bytes) > 1024 * 1024 - total) return false;
		std::vector<char> value(static_cast<size_t>(bytes));
		if (fgetxattr(source, name, value.data(), value.size()) != bytes ||
			fsetxattr(target, name, value.data(), value.size(), 0) != 0) return false;
		total += value.size(); cursor += length + 1;
	}
	return true;
#endif
}
#endif
}

bool ScriptConfig::ReadCipherSnapshot(std::string& bytes)
{
	std::lock_guard<std::mutex> lock(m_configWriteMutex);
#ifndef NZBGET_CIPHER_ATOMIC_CONFIG
	return false; // Unsupported atomic metadata-preserving platform adapter.
#else
	CipherDescriptor file(open(g_Options->GetConfigFilename(), O_RDONLY | O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC));
	struct stat metadata;
	return file.value >= 0 && ReadCipherDescriptor(file.value, bytes, metadata);
#endif
}

ScriptConfig::CipherCommit ScriptConfig::CommitCipherPatch(std::string_view expected, std::string_view replacement,
	const std::function<bool()>& cancel)
{
	std::lock_guard<std::mutex> lock(m_configWriteMutex);
	if (expected.size() > 16 * 1024 * 1024 || replacement.size() > 16 * 1024 * 1024) return CipherCommit::Failed;
	if (cancel()) return CipherCommit::Cancelled;
#ifndef NZBGET_CIPHER_ATOMIC_CONFIG
	return CipherCommit::Failed;
#else
	fs::path filename(g_Options->GetConfigFilename());
	std::string leaf = filename.filename().string();
	CipherDescriptor parent(open(filename.parent_path().empty() ? "." : filename.parent_path().string().c_str(), O_RDONLY | O_DIRECTORY | O_CLOEXEC));
	if (parent.value < 0 || leaf.empty()) return CipherCommit::Failed;
	CipherDescriptor original(openat(parent.value, leaf.c_str(), O_RDONLY | O_NOFOLLOW | O_NONBLOCK | O_CLOEXEC));
	std::string current;
	struct stat before;
	if (original.value < 0 || !ReadCipherDescriptor(original.value, current, before)) return CipherCommit::Failed;
	if (current != expected) return CipherCommit::Conflict;
	if (current == replacement) return CipherCommit::Unchanged;
	static std::atomic<uint64_t> sequence{0};
	std::string temporary;
	int descriptor = -1;
	for (int attempt = 0; attempt < 16; ++attempt)
	{
		temporary = "." + leaf + ".tls-" + std::to_string(getpid()) + "-" + std::to_string(sequence.fetch_add(1));
		descriptor = openat(parent.value, temporary.c_str(), O_WRONLY | O_CREAT | O_EXCL | O_NOFOLLOW | O_CLOEXEC, 0600);
		if (descriptor >= 0 || errno != EEXIST) break;
	}
	if (descriptor < 0) return CipherCommit::Failed;
	CipherDescriptor output(descriptor);
	struct RemoveTemporary
	{
		int parent; const std::string& name;
		~RemoveTemporary() { unlinkat(parent, name.c_str(), 0); }
	} cleanup{parent.value, temporary};
	size_t used = 0;
	while (used < replacement.size())
	{
		if (cancel()) return CipherCommit::Cancelled;
		ssize_t count = write(output.value, replacement.data() + used, replacement.size() - used);
		if (count < 0 && errno == EINTR) continue;
		if (count <= 0) return CipherCommit::Failed;
		used += static_cast<size_t>(count);
	}
	if (!CopyCipherAttributes(original.value, output.value) || fchown(output.value, before.st_uid, before.st_gid) != 0 ||
		fchmod(output.value, before.st_mode & 07777) != 0 || fsync(output.value) != 0) return CipherCommit::Failed;
	int closeResult = close(output.value); output.value = -1;
	if (closeResult != 0) return CipherCommit::Failed;
	struct stat after, entry;
	std::string rechecked;
	if (!ReadCipherDescriptor(original.value, rechecked, after) || !SameCipherFile(before, after) || rechecked != expected ||
		fstatat(parent.value, leaf.c_str(), &entry, AT_SYMLINK_NOFOLLOW) != 0 || !SameCipherFile(before, entry)) return CipherCommit::Conflict;
	if (cancel()) return CipherCommit::Cancelled;
	if (renameat(parent.value, temporary.c_str(), parent.value, leaf.c_str()) != 0) return CipherCommit::Failed;
	// The replacement has happened. A late cancel or directory-sync failure must
	// never be reported as "unchanged"; retain the truthful committed state.
	if (fsync(parent.value) != 0) warn("TLS cipher settings were replaced, but the configuration directory could not be synchronized");
	return CipherCommit::Saved;
#endif
}

void ScriptConfig::InitOptions()
{
	InitConfigTemplates();
}

bool ScriptConfig::LoadConfig(Options::OptEntries* optEntries)
{
	std::lock_guard<std::mutex> lock(m_configWriteMutex);
	return LoadConfigUnlocked(optEntries);
}

bool ScriptConfig::LoadConfigUnlocked(Options::OptEntries* optEntries)
{
	if (!optEntries || !g_Options || !g_Options->GetConfigFilename())
	{
		return false;
	}
	return nzbget_rs_script_config_load(
		g_Options->GetConfigFilename(),
		optEntries,
		[](void* ctx, const char* name, const char* value)
		{
			static_cast<Options::OptEntries*>(ctx)->emplace_back(name, value);
		}) != 0;
}

// Versioned, unambiguous saved-value token shared with Config.cipherBaseline.
// Missing fields differ from present empty fields; duplicate keys are last-wins.
std::string ScriptConfig::CipherBaseline(const Options::OptEntries& optEntries)
{
	std::vector<const char*> names;
	std::vector<const char*> values;
	names.reserve(optEntries.size());
	values.reserve(optEntries.size());
	for (const auto& entry : optEntries)
	{
		if (!entry.GetName() || !entry.GetValue()) continue;
		names.push_back(entry.GetName());
		values.push_back(entry.GetValue());
	}
	NzbgetRsBuf buf = nzbget_rs_sus_build_cipher_baseline(names.data(), values.data(), names.size());
	std::string token(buf.data ? buf.data : "", buf.len);
	nzbget_rs_free(buf);
	return token;
}

bool ScriptConfig::SaveConfig(Options::OptEntries* optEntries, const char* cipherBaseline, bool* conflict)
{
	std::lock_guard<std::mutex> lock(m_configWriteMutex);
	if (conflict) *conflict = false;
	if (!optEntries || !g_Options || !g_Options->GetConfigFilename())
	{
		return false;
	}
	std::vector<const char*> names;
	std::vector<const char*> values;
	names.reserve(optEntries->size());
	values.reserve(optEntries->size());
	for (const auto& entry : *optEntries)
	{
		if (entry.GetName() && entry.GetValue())
		{
			names.push_back(entry.GetName());
			values.push_back(entry.GetValue());
		}
	}
	int conflictFlag = 0;
	int saved = nzbget_rs_script_config_save(
		g_Options->GetConfigFilename(),
		names.data(),
		values.data(),
		names.size(),
		cipherBaseline,
		&conflictFlag);
	if (conflict)
	{
		*conflict = conflictFlag != 0;
	}
	return saved != 0;
}

bool ScriptConfig::LoadConfigTemplates(ConfigTemplates* configTemplates)
{
	CharBuffer buffer;
	if (!FileSystem::LoadFileIntoBuffer(g_Options->GetConfigTemplate(), buffer, true))
	{
		return false;
	}
	configTemplates->emplace_back(ConfigTemplate(buffer));

	return true;
}

void ScriptConfig::InitConfigTemplates()
{
	if (!LoadConfigTemplates(&m_configTemplates))
	{
		error("Could not read configuration templates");
	}
}
