/*
 *  This file is part of RustyGet.
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


#ifndef RARRENAMER_H
#define RARRENAMER_H

#include <exception>
#include "NString.h"
#include "Log.h"
#include "FileSystem.h"
#include "nzbget_rs.h"

class RarRenamer
{
public:
	void Execute()
	{
		struct Context
		{
			RarRenamer* self;
			std::exception_ptr error;

			int Fail()
			{
				error = std::current_exception();
				return -1;
			}
		} context{this, nullptr};

		NzbgetRsRarRenamerRunEnv env{};
		env.ctx = &context;
		env.stopped = +[](void* ctx) noexcept -> int
		{
			Context& c = *static_cast<Context*>(ctx);
			if (c.error) return -1;
			try { return c.self->IsStopped() ? 1 : 0; } catch (...) { return c.Fail(); }
		};
		env.progress = +[](void* ctx, const char* label, int stage) noexcept -> int
		{
			Context& c = *static_cast<Context*>(ctx);
			if (c.error) return -1;
			try
			{
				c.self->m_progressLabel = label;
				c.self->m_stageProgress = stage;
				c.self->UpdateProgress();
				return 0;
			}
			catch (...) { return c.Fail(); }
		};
		env.message = +[](void* ctx, int kind, const char* text) noexcept -> int
		{
			Context& c = *static_cast<Context*>(ctx);
			if (c.error) return -1;
			try
			{
				c.self->PrintMessage(
					kind == 0 ? Message::mkInfo : kind == 1 ? Message::mkWarning : Message::mkError,
					"%s", text);
				return 0;
			}
			catch (...) { return c.Fail(); }
		};
		env.registerRenamed = +[](void* ctx, const char* oldBase, const char* newBase) noexcept -> int
		{
			Context& c = *static_cast<Context*>(ctx);
			if (c.error) return -1;
			try
			{
				c.self->RegisterRenamedFile(oldBase, newBase);
				return 0;
			}
			catch (...) { return c.Fail(); }
		};

#ifndef DISABLE_TLS
		const int tlsEnabled = 1;
#else
		const int tlsEnabled = 0;
#endif

		nzbget_rs_rar_renamer_execute(
			&env, *m_destDir, *m_infoName, *m_password, *m_ignoreExt, tlsEnabled, &m_renamedCount);

		if (context.error)
		{
			std::rethrow_exception(context.error);
		}
	}

	void SetDestDir(const char* destDir) { m_destDir = destDir; }
	const char* GetInfoName() { return m_infoName; }
	void SetInfoName(const char* infoName) { m_infoName = infoName; }
	void SetPassword(const char* password) { m_password = password; }
	void SetIgnoreExt(const char* ignoreExt) { m_ignoreExt = ignoreExt; }
	int GetRenamedCount() { return m_renamedCount; }

protected:
	virtual void UpdateProgress() {}
	virtual bool IsStopped() { return false; };
	virtual void PrintMessage([[maybe_unused]] Message::EKind kind, [[maybe_unused]] const char* format,
		...) PRINTF_SYNTAX(3) {}
	virtual void RegisterRenamedFile([[maybe_unused]] const char* oldFilename,
		[[maybe_unused]] const char* newFileName) {}
	const char* GetProgressLabel() { return m_progressLabel; }
	int GetStageProgress() { return m_stageProgress; }

private:
	CString m_infoName;
	CString m_destDir;
	CString m_progressLabel;
	int m_stageProgress = 0;
	int m_renamedCount = 0;
	CString m_password;
	CString m_ignoreExt;
};

#endif
