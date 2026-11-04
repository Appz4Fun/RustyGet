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

#include <fstream>
#include <sstream>
#include <type_traits>
#include "DeadPostings.h"
#include "DupeUtil.h"
#include "FileSystem.h"
#include "Log.h"
#include "Util.h"
#include "nzbget_rs.h"

void DeadPostings::SetStatePath(const std::string& path)
{
	std::lock_guard<std::mutex> guard(m_mutex);
	m_statePath = path;
}

void DeadPostings::Load()
{
	std::lock_guard<std::mutex> guard(m_mutex);
	std::ifstream file(fs::u8path(m_statePath));
	std::string line;
	while (std::getline(file, line))
	{
		nzbget_rs_deadpostings_parse(reinterpret_cast<const uint8_t*>(line.data()), line.size(), &m_dead,
			[](void* context, int64_t stamp, const uint32_t* hashes, size_t count) noexcept
			{
				auto* entries = static_cast<decltype(m_dead)*>(context);
				entries->emplace_back((time_t)stamp, Posting::Sketch(hashes, hashes + count));
			});
	}
	Prune(Util::CurrentTime());
}

// with m_mutex held
void DeadPostings::Prune(time_t now)
{
	auto stale = [now](const std::pair<time_t, Posting::Sketch>& entry)
		{
			return nzbget_rs_deadpostings_is_stale((int64_t)now, (int64_t)entry.first) != 0;
		};
	m_dead.erase(std::remove_if(m_dead.begin(), m_dead.end(), stale), m_dead.end());
	m_unsaved.erase(std::remove_if(m_unsaved.begin(), m_unsaved.end(), stale), m_unsaved.end());
}

// with m_mutex held: a temporary file, renamed, so a crash can't leave half of the file
void DeadPostings::Save()
{
	if (m_statePath.empty())
	{
		return;
	}
	std::string text;
	for (const auto& entry : m_dead)
	{
		NzbgetRsBuf buf = nzbget_rs_deadpostings_format_line(
			(int64_t)entry.first, entry.second.data(), entry.second.size());
		if (buf.data && buf.len > 0)
		{
			text.append(buf.data, buf.len);
		}
		nzbget_rs_free(buf);
	}
	if (!DupeUtil::WriteAtomic(m_statePath, text))
	{
		warn("Could not save the DupeSearch dead postings to %s", m_statePath.c_str());
	}
}

bool DeadPostings::IsDead(const Posting::Sketch& sketch, time_t now)
{
	std::lock_guard<std::mutex> guard(m_mutex);
	now = now ? now : Util::CurrentTime();
	for (const auto* list : { &m_dead, &m_unsaved })
	{
		for (const auto& entry : *list)
		{
			if (nzbget_rs_deadpostings_entry_matches(
				(int64_t)now, (int64_t)entry.first,
				sketch.data(), sketch.size(),
				entry.second.data(), entry.second.size()) != 0)
			{
				return true;
			}
		}
	}
	return false;
}

void DeadPostings::Add(const Posting::Sketch& sketch, time_t now, bool save)
{
	std::lock_guard<std::mutex> guard(m_mutex);
	now = now ? now : Util::CurrentTime();
	Prune(now);
	(save ? m_dead : m_unsaved).emplace_back(now, sketch);
	if (save)
	{
		Save();
	}
}
