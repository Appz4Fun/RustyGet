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
#include "Util.h"
#include "LoggableFrontend.h"
#include "Log.h"
#include "nzbget_rs.h"

void LoggableFrontend::Run()
{
	debug("Entering LoggableFrontend-loop");

	while (!IsStopped())
	{
		Update();
		Wait(m_updateInterval);
	}
	// Printing the last messages
	Update();

	BeforeExit();

	debug("Exiting LoggableFrontend-loop");
}

void LoggableFrontend::Update()
{
	if (!PrepareData())
	{
		FreeData();
		return;
	}

	BeforePrint();

	{
		GuardedMessageList messages = GuardMessages();
		if (!messages->empty())
		{
			Message& firstMessage = messages->front();
			int skip = 0;
			int start = nzbget_rs_feh_frontend_calc_log_start_index(
				m_neededLogFirstId, firstMessage.GetId(), &skip);
			if (skip)
			{
				PrintSkip();
			}
			for (uint32 i = (uint32)start; i < messages->size(); i++)
			{
				PrintMessage(messages->at(i));
				m_neededLogFirstId = messages->at(i).GetId();
			}
		}
	}

	PrintStatus();

	FreeData();

	fflush(stdout);
}

void LoggableFrontend::PrintMessage(Message& message)
{
	NzbgetRsBuf line = nzbget_rs_frontend_format_log_message(
		0, (int)message.GetKind(), message.GetText());
	if (line.data)
	{
		fwrite(line.data, 1, line.len, stdout);
	}
	nzbget_rs_free(line);
}

void LoggableFrontend::PrintSkip()
{
	printf(".....\n");
}
