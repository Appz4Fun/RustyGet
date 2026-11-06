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
 *  along with this program.  If not, see <https://www.gnu.org/licenses/>.
 */


#include "nzbget.h"

#include "DeadPostings.h"
#include "Log.h"
#include "Util.h"
#include "nzbget_rs.h"

DeadPostings::DeadPostings()
	: m_store(nzbget_rs_deadpostings_store_new())
{
}

DeadPostings::~DeadPostings()
{
	nzbget_rs_deadpostings_store_free(m_store);
}

void DeadPostings::SetStatePath(const std::string& path)
{
	m_statePath = path;
	nzbget_rs_deadpostings_store_set_state_path(
		m_store, reinterpret_cast<const uint8_t*>(path.data()), path.size());
}

void DeadPostings::Load()
{
	nzbget_rs_deadpostings_store_load(m_store, (int64_t)Util::CurrentTime());
}

bool DeadPostings::IsDead(const Posting::Sketch& sketch, time_t now)
{
	now = now ? now : Util::CurrentTime();
	return nzbget_rs_deadpostings_store_is_dead(
		m_store, sketch.data(), sketch.size(), (int64_t)now) != 0;
}

void DeadPostings::Add(const Posting::Sketch& sketch, time_t now, bool save)
{
	now = now ? now : Util::CurrentTime();
	if (nzbget_rs_deadpostings_store_add(
		m_store, sketch.data(), sketch.size(), (int64_t)now, save ? 1 : 0) == 0)
	{
		warn("Could not save the DupeSearch dead postings to %s", m_statePath.c_str());
	}
}
