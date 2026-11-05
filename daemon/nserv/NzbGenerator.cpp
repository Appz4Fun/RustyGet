/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2016 Andrey Prygunkov <hugbug@users.sourceforge.net>
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
#include "NzbGenerator.h"
#include "Log.h"
#include "nzbget_rs.h"

void NzbGenerator::Execute()
{
	// rust/src/nserv.rs
	auto logCb = [](int kind, const char* msg)
	{
		switch (kind)
		{
		case 0:
			info("Generating nzbs for %s", msg);
			break;
		case 1:
			info("Generating nzb for %s", msg);
			break;
		case 2:
			error("Could not create file %s", msg);
			break;
		case 3:
			detail("Processing %s", msg);
			break;
		case 4:
			info("Nzb generation finished");
			break;
		}
	};
	nzbget_rs_nzb_generator_execute(m_dataDir, m_segmentSize, logCb);
}
