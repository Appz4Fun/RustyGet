/*
 *  This file is part of RustyGet.
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


#ifndef LOGGABLEFRONTEND_H
#define LOGGABLEFRONTEND_H

#include <cstdio>
#include "Frontend.h"
#include "Log.h"
#include "nzbget_rs.h"

class LoggableFrontend : public Frontend
{
protected:
	virtual void Run()
	{
		debug("Entering LoggableFrontend-loop");

		while (!IsStopped())
		{
			Update();
			Wait(m_updateInterval);
		}
		Update();

		BeforeExit();

		debug("Exiting LoggableFrontend-loop");
	}

	virtual void Update()
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

	virtual void BeforePrint() {}
	virtual void BeforeExit() {}

	virtual void PrintMessage(Message& message)
	{
		NzbgetRsBuf line = nzbget_rs_frontend_format_log_message(
			0, (int)message.GetKind(), message.GetText());
		if (line.data)
		{
			fwrite(line.data, 1, line.len, stdout);
		}
		nzbget_rs_free(line);
	}

	virtual void PrintStatus() {}

	virtual void PrintSkip()
	{
		printf(".....\n");
	}
};

#endif
