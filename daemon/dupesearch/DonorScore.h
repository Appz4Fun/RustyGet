/*
 *  This file is part of RustyGet.
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


#ifndef DONORSCORE_H
#define DONORSCORE_H

#include <set>
#include <vector>
#include "nzbget_rs.h"

class DonorScore
{
public:
	static constexpr int Dead = 1;
	static constexpr int Top = 90;

	static int Base(int pickScore)
	{
		return nzbget_rs_donorscore_base(pickScore);
	}

	static int Target(double alive, bool twin)
	{
		return nzbget_rs_donorscore_target(alive, twin ? 1 : 0);
	}

	class Ranks
	{
	public:
		int Take(double alive, bool twin)
		{
			std::vector<int32_t> used(m_used.begin(), m_used.end());
			int score = nzbget_rs_donorscore_take_rank(alive, twin ? 1 : 0, used.data(), used.size());
			m_used.insert(score);
			return score;
		}
		void Release(int score) { m_used.erase(score); }
		void Use(int score) { m_used.insert(score); }
	private:
		std::set<int> m_used;
	};

	struct Entry
	{
		int score = 0;
		double alive = -1;
		bool twin = false;
		long long bytes = 0;
		int grabs = 0;
	};

	static std::vector<int> Rerank(const std::vector<Entry>& entries, long long pickBytes)
	{
		std::vector<NzbgetRsDonorEntry> raw;
		raw.reserve(entries.size());
		for (const Entry& e : entries)
		{
			raw.push_back({e.score, e.alive, e.twin ? 1 : 0, (int64_t)e.bytes, e.grabs});
		}
		std::vector<int> scores(entries.size());
		nzbget_rs_donorscore_rerank(raw.data(), raw.size(), (int64_t)pickBytes, scores.data());
		return scores;
	}
};

#endif
