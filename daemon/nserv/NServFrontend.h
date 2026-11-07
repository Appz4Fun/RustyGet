/*
 *  This file is part of RustyGet.
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


#ifndef NSERVFRONTEND_H
#define NSERVFRONTEND_H

#include <cstdio>
#include "Thread.h"
#include "Log.h"
#include "Util.h"
#include "nzbget_rs.h"

class NServFrontend : public Thread
{
public:
	NServFrontend() = default;

private:
	uint32 m_neededLogEntries = 0;
	uint32 m_neededLogFirstId = 0;
	bool m_needGoBack = false;

	void Run()
	{
		while (!IsStopped())
		{
			Update();
			Util::Sleep(100);
		}
		Update();
	}

	void Update()
	{
		BeforePrint();

		{
			GuardedMessageList messages = g_Log->GuardMessages();
			if (!messages->empty())
			{
				Message& firstMessage = messages->front();
				int needSkip = 0;
				int start = nzbget_rs_nserv_frontend_calc_start_index(
					m_neededLogFirstId, firstMessage.GetId(), &needSkip);
				if (needSkip)
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

		fflush(stdout);
	}

	void BeforePrint()
	{
		if (m_needGoBack)
		{
			printf("\r\033[1A");
			m_needGoBack = false;
		}
	}

	void PrintMessage(Message& message)
	{
		NzbgetRsBuf line = nzbget_rs_frontend_format_log_message(
			1, (int)message.GetKind(), message.GetText());
		if (line.data)
		{
			fwrite(line.data, 1, line.len, stdout);
		}
		nzbget_rs_free(line);
	}

	void PrintSkip()
	{
		printf(".....\033[K\n");
	}
};

#endif
