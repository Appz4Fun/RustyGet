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

#include <algorithm>
#include <cmath>
#include <numeric>
#include "DonorScore.h"
#include "nzbget_rs.h"

// rust/src/donorscore.rs
int DonorScore::Base(int pickScore)
{
	return nzbget_rs_donorscore_base(pickScore);
}

int DonorScore::Target(double alive, bool twin)
{
	return nzbget_rs_donorscore_target(alive, twin ? 1 : 0);
}

int DonorScore::Ranks::Take(double alive, bool twin)
{
	std::vector<int32_t> used(m_used.begin(), m_used.end());
	int score = nzbget_rs_donorscore_take_rank(alive, twin ? 1 : 0, used.data(), used.size());
	m_used.insert(score);
	return score;
}

std::vector<int> DonorScore::Rerank(const std::vector<Entry>& entries, long long pickBytes)
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
