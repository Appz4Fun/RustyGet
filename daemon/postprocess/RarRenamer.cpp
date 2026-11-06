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

void RarRenamer::Execute()
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
	env.stopped = [](void* ctx) noexcept -> int
	{
		Context& c = *static_cast<Context*>(ctx);
		if (c.error) return -1;
		try { return c.self->IsStopped() ? 1 : 0; } catch (...) { return c.Fail(); }
	};
	env.progress = [](void* ctx, const char* label, int stage) noexcept -> int
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
	env.message = [](void* ctx, int kind, const char* text) noexcept -> int
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
	env.registerRenamed = [](void* ctx, const char* oldBase, const char* newBase) noexcept -> int
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
