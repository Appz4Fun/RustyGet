/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2004 Sven Henkel <sidddy@users.sourceforge.net>
 *  Copyright (C) 2007-2016 Andrey Prygunkov <hugbug@users.sourceforge.net>
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
#include "Observer.h"
#include "Log.h"
#ifdef NZBGET_USE_RUST
#include "nzbget_rs.h"
#endif

void Subject::Attach(Observer* observer)
{
	m_observers.push_back(observer);
}

void Subject::Detach(Observer* observer)
{
#ifdef NZBGET_USE_RUST
	std::vector<uintptr_t> ptrs(m_observers.size());
	for (size_t i = 0; i < m_observers.size(); ++i)
	{
		ptrs[i] = reinterpret_cast<uintptr_t>(m_observers[i]);
	}
	ptrdiff_t idx = nzbget_rs_awc_observer_find_index(
		ptrs.data(), ptrs.size(), reinterpret_cast<uintptr_t>(observer));
	if (idx >= 0)
	{
		m_observers.erase(m_observers.begin() + idx);
	}
#else
	// one that isn't attached: erasing end() is undefined
	auto it = std::find(m_observers.begin(), m_observers.end(), observer);
	if (it != m_observers.end())
	{
		m_observers.erase(it);
	}
#endif
}

void Subject::Notify(void* aspect)
{
	debug("Notifying observers");

	for (Observer* observer : m_observers)
	{
		observer->Update(this, aspect);
	}
}
