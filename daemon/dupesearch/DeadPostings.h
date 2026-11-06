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


#ifndef DEADPOSTINGS_H
#define DEADPOSTINGS_H

#include <ctime>
#include <string>
#include "Posting.h"

struct NzbgetRsDeadPostingsStore;

/*
 * Postings found dead (their articles are gone from the news servers), by
 * sketch, kept for DeadTtlSec: taken-down articles don't come back, and
 * checking the same posting again on every search would cost a grab and
 * server load for nothing. A near-identical re-listing of a dead posting
 * (the same articles with one re-uploaded) counts as the same posting.
 * Kept across restarts in a file that is replaced atomically.
 */
class DeadPostings
{
public:
	static constexpr int DeadTtlSec = 3 * 24 * 3600;

	DeadPostings();
	~DeadPostings();
	DeadPostings(const DeadPostings&) = delete;
	DeadPostings& operator=(const DeadPostings&) = delete;

	void SetStatePath(const std::string& path);
	void Load();

	bool IsDead(const Posting::Sketch& sketch, time_t now = 0);
	/* save false: counts from now on, but is never written (a dry run's verdicts) */
	void Add(const Posting::Sketch& sketch, time_t now = 0, bool save = true);

private:
	NzbgetRsDeadPostingsStore* m_store;
	std::string m_statePath;
};

#endif
