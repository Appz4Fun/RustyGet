/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2016 Andrey Prygunkov <hugbug@users.sourceforge.net>
 *  Copyright (C) 2024-2025 Denis <denis@nzbget.com>
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


#ifndef RARREADER_H
#define RARREADER_H

#include <exception>
#include <string>
#include "Log.h"
#include "Util.h"
#include "FileSystem.h"
#include "ContentMap.h"
#include "nzbget_rs.h"

class RarFile
{
public:
	// rar5 per-file encryption parameters (FHEXTRA_CRYPT record, type 0x01).
	// The on-disk field order and sizes are fixed by the RAR5 format; see
	// RarVolume::ReadRar5Crypt. CheckValue holds the raw 12 bytes (8-byte
	// password check + 4-byte checksum) and is meaningful only if HasCheck.
	struct Rar5Crypt
	{
		uint64 Version = 0;
		uint64 Flags = 0;
		uint8 KdfCount = 0;
		uint8 Salt[16] = {};
		uint8 Iv[16] = {};
		uint8 CheckValue[12] = {};
		bool HasCheck = false;
	};

	const char* GetFilename() const { return m_filename.c_str(); }
	uint32 GetTime() { return m_time; }
	uint32 GetAttr() { return m_attr; }
	int64 GetSize() { return m_size; }
	bool GetSplitBefore() { return m_splitBefore; }
	bool GetSplitAfter() { return m_splitAfter; }
	uint32 GetMethod() { return m_method; }
	bool GetStored() { return m_stored; }
	int64 GetPackedSize() { return m_packedSize; }
	int64 GetDataOffset() { return m_dataOffset; }
	bool GetEncryptedData() { return m_encryptedData; }
	// rar3: the 8-byte per-file salt stored after the filename when the file
	// header carries the SALT flag (0x0400). Valid only if GetHasSalt().
	bool GetHasSalt() { return m_hasSalt; }
	const uint8* GetSalt() { return m_salt; }
	// rar5: the parsed file-crypt record, or nullptr when the file has none.
	const Rar5Crypt* GetCrypt() { return m_hasCrypt ? &m_crypt : nullptr; }
private:
	std::string m_filename;
	uint32 m_time = 0;
	uint32 m_attr = 0;
	int64 m_size = 0;
	bool m_splitBefore = false;
	bool m_splitAfter = false;
	uint32 m_method = 0;
	bool m_stored = false;
	int64 m_packedSize = 0;
	int64 m_dataOffset = -1;
	bool m_encryptedData = false;
	bool m_hasSalt = false;
	uint8 m_salt[8] = {};
	bool m_hasCrypt = false;
	Rar5Crypt m_crypt;
	friend class RarVolume;
};

class RarVolume
{
public:
	using FileList = std::deque<RarFile>;

	explicit RarVolume(const char* filename) 
		: m_filename{ filename ? filename : "" }
		, m_decryptKey{}
		, m_decryptIV{}
		, m_decryptBuf{}
		{}

	bool Read()
	{
		debug("Checking file %s", m_filename.c_str());

		DiskFile file;
		if (!file.Open(m_filename.c_str(), DiskFile::omRead))
		{
			return false;
		}

		// size the source through the open handle: a path-based stat could race
		// a concurrent rename/replace and disagree with the handle's file
		int64 size = file.Seek(0, DiskFile::soEnd) ? file.Position() : -1;
		if (size < 0 || !file.Seek(0))
		{
			file.Close();
			return false;
		}

		DiskContentSource source(file, size);
		bool ok = ReadFrom(source);
		file.Close();
		return ok;
	}

	bool ReadFrom(ContentSource& source)
	{
		// rust/src/rarreader.rs parses the headers; the content source and the
		// cipher (OpenSSL) stay here. A callback that throws makes the parse fail;
		// the exception is rethrown once Rust has returned.
		RarContext context{this, &source, nullptr};

		NzbgetRsRarIo io{};
		io.ctx = &context;
		io.size = [](void* ctx) -> int64_t
		{
			return RarCall(ctx, (int64_t)-1, [](RarContext* c) { return (int64_t)c->source->Size(); });
		};
		io.read = [](void* ctx, int64_t offset, uint8_t* buffer, int64_t len) -> int
		{
			return RarCall(ctx, 0, [=](RarContext* c)
			{
				bool ok = c->source->Read(offset, buffer, len);
				c->signatureRead = true;
				return ok ? 1 : 0;
			});
		};
		io.addFile = [](void* ctx, const NzbgetRsRarFile* f) -> int
		{
			return RarCall(ctx, 0, [f](RarContext* c)
			{
				RarFile file;
				file.m_filename.assign(f->filename, f->filenameLen);
				file.m_time = f->time;
				file.m_attr = f->attr;
				file.m_size = f->size;
				file.m_splitBefore = f->splitBefore;
				file.m_splitAfter = f->splitAfter;
				file.m_method = f->method;
				file.m_stored = f->stored;
				file.m_packedSize = f->packedSize;
				file.m_dataOffset = f->dataOffset;
				file.m_encryptedData = f->encryptedData;
				file.m_hasSalt = f->hasSalt;
				memcpy(file.m_salt, f->salt, sizeof(file.m_salt));
				file.m_hasCrypt = f->hasCrypt;
				if (f->hasCrypt)
				{
					file.m_crypt.Version = f->cryptVersion;
					file.m_crypt.Flags = f->cryptFlags;
					file.m_crypt.KdfCount = f->cryptKdfCount;
					memcpy(file.m_crypt.Salt, f->cryptSalt, sizeof(file.m_crypt.Salt));
					memcpy(file.m_crypt.Iv, f->cryptIv, sizeof(file.m_crypt.Iv));
					memcpy(file.m_crypt.CheckValue, f->cryptCheckValue, sizeof(file.m_crypt.CheckValue));
					file.m_crypt.HasCheck = f->cryptHasCheck;
				}
				c->volume->m_files.push_back(std::move(file));
				return 1;
			});
		};

		io.setDataOffset = [](void* ctx, int64_t position)
		{
			RarCall(ctx, 0, [=](RarContext* c)
			{
				RarFile& file = c->volume->m_files.back();
				// Preserve the two's-complement result without signed overflow.
				file.m_dataOffset = (int64)((uint64)position - (uint64)file.m_packedSize);
				return 0;
			});
		};

		NzbgetRsRarVolume state{};
		state.version = m_version;
		state.volumeNo = m_volumeNo;
		state.newNaming = m_newNaming;
		state.hasNextVolume = m_hasNextVolume;
		state.multiVolume = m_multiVolume;
		state.encrypted = m_encrypted;
		state.hasPassword = !m_password.empty();
		memcpy(state.decryptBuf, m_decryptBuf, sizeof(m_decryptBuf));
		state.decryptPos = m_decryptPos;
#ifndef DISABLE_TLS
		state.useNativeCipher = 1;
		state.password = m_password.c_str();
		state.passwordLen = m_password.size();
		memcpy(state.decryptKey, m_decryptKey, sizeof(m_decryptKey));
		memcpy(state.decryptIv, m_decryptIV, sizeof(m_decryptIV));
#endif

		bool ok = nzbget_rs_rar_read(&io, &state) != 0;

		// DetectRarVersion never assigned m_version if Size/Read threw while
		// reading the signature. Preserve it when this is a repeated ReadFrom.
		if (!context.error || context.signatureRead)
		{
			m_version = state.version;
		}
		m_volumeNo = state.volumeNo;
		m_newNaming = state.newNaming;
		m_hasNextVolume = state.hasNextVolume;
		m_multiVolume = state.multiVolume;
		m_encrypted = state.encrypted;
		memcpy(m_decryptBuf, state.decryptBuf, sizeof(m_decryptBuf));
		m_decryptPos = state.decryptPos;
#ifndef DISABLE_TLS
		memcpy(m_decryptKey, state.decryptKey, sizeof(m_decryptKey));
		memcpy(m_decryptIV, state.decryptIv, sizeof(m_decryptIV));
#endif

		if (context.error)
		{
			std::rethrow_exception(context.error);
		}

		LogDebugInfo();

		return ok;
	}

	const char* GetFilename() const { return m_filename.c_str(); }
	int GetVersion() { return m_version; }
	uint32 GetVolumeNo() { return m_volumeNo; }
	bool GetNewNaming() { return m_newNaming; }
	bool GetHasNextVolume() { return m_hasNextVolume; }
	bool GetMultiVolume() { return m_multiVolume; }
	bool GetEncrypted() { return m_encrypted; }
	void SetPassword(const char* password) { m_password = password ? password : ""; }
	FileList* GetFiles() { return &m_files; }

	static void LogDebugVolume(const char* filename, int version, bool multiVolume,
		uint32 volumeNo, bool newNaming, bool hasNextVolume, bool encrypted,
		size_t fileCount, const uint32* fileTimes, const int64* fileSizes,
		const uint32* fileAttrs, const uint8* fileSplitBefore,
		const uint8* fileSplitAfter, const char* const* fileNames)
	{
#ifdef DEBUG
		RarVolume vol(filename);
		vol.m_version = version;
		vol.m_multiVolume = multiVolume;
		vol.m_volumeNo = volumeNo;
		vol.m_newNaming = newNaming;
		vol.m_hasNextVolume = hasNextVolume;
		vol.m_encrypted = encrypted;
		for (size_t i = 0; i < fileCount; i++)
		{
			RarFile file;
			file.m_filename = (fileNames && fileNames[i]) ? fileNames[i] : "";
			file.m_time = fileTimes ? fileTimes[i] : 0;
			file.m_size = fileSizes ? fileSizes[i] : 0;
			file.m_attr = fileAttrs ? fileAttrs[i] : 0;
			file.m_splitBefore = fileSplitBefore && fileSplitBefore[i] != 0;
			file.m_splitAfter = fileSplitAfter && fileSplitAfter[i] != 0;
			vol.m_files.push_back(std::move(file));
		}
		vol.LogDebugInfo();
#else
		(void)filename;
		(void)version;
		(void)multiVolume;
		(void)volumeNo;
		(void)newNaming;
		(void)hasNextVolume;
		(void)encrypted;
		(void)fileCount;
		(void)fileTimes;
		(void)fileSizes;
		(void)fileAttrs;
		(void)fileSplitBefore;
		(void)fileSplitAfter;
		(void)fileNames;
#endif
	}

private:
	struct RarContext
	{
		RarVolume* volume;
		ContentSource* source;
		std::exception_ptr error;
		bool signatureRead = false;
	};

	template <typename R, typename F>
	static R RarCall(void* ctx, R failed, F f) noexcept
	{
		RarContext* c = static_cast<RarContext*>(ctx);
		if (c->error) return failed;
		try
		{
			return f(c);
		}
		catch (...)
		{
			c->error = std::current_exception();
			return failed;
		}
	}

	FileList m_files;

	std::string m_filename;
	std::string m_password;

	bool m_newNaming = false;
	bool m_hasNextVolume = false;
	bool m_multiVolume = false;
	bool m_encrypted = false;

	int m_version = 0;
	uint32 m_volumeNo = 0;
	uint8 m_decryptKey[32];
	uint8 m_decryptIV[16];
	uint8 m_decryptBuf[16];
	uint8 m_decryptPos = 16;

	void LogDebugInfo()
	{
#ifdef DEBUG
		debug("Volume: version:%i, multi:%i, vol-no:%i, new-naming:%i, has-next:%i, encrypted:%i, file-count:%i, [%s]",
			(int)m_version, (int)m_multiVolume, m_volumeNo, (int)m_newNaming, (int)m_hasNextVolume,
			(int)m_encrypted, (int)m_files.size(), FileSystem::BaseFileName(m_filename.c_str()));

		for (RarFile& file : m_files)
		{
			debug("  time:%i, size:%" PRIi64 ", attr:%i, split-before:%i, split-after:%i, [%s]",
				file.m_time, file.m_size, file.m_attr,
				file.m_splitBefore, file.m_splitAfter, file.m_filename.c_str());
		}
#endif
	}
};

#endif
