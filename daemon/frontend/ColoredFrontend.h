/*
 *  This file is part of RustyGet.
 *
 *  Copyright (C) 2004 Sven Henkel <sidddy@users.sourceforge.net>
 *  Copyright (C) 2007-2016 Andrey Prygunkov <hugbug@users.sourceforge.net>
 *  Copyright (C) 2024 Denis <denis@nzbget.com>
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


#ifndef COLOREDFRONTEND_H
#define COLOREDFRONTEND_H

#include <cstdio>
#include "LoggableFrontend.h"
#include "Log.h"
#include "nzbget_rs.h"

class ColoredFrontend : public LoggableFrontend
{
public:
	ColoredFrontend()
	{
		m_summary = true;
	}

protected:
	virtual void BeforePrint()
	{
		if (m_needGoBack)
		{
			printf("\r\033[1A");
			m_needGoBack = false;
		}
	}

	virtual void PrintMessage(Message& message)
	{
		NzbgetRsBuf line = nzbget_rs_frontend_format_log_message(
			1, (int)message.GetKind(), message.GetText());
		if (line.data)
		{
			fwrite(line.data, 1, line.len, stdout);
		}
		nzbget_rs_free(line);
	}

	virtual void PrintStatus()
	{
		printf("\033[s");
		NzbgetRsBuf status = nzbget_rs_colored_frontend_format_status_bar(
			m_standBy ? 1 : 0,
			m_pauseDownload ? 1 : 0,
			m_currentDownloadSpeed,
			m_remainingSize,
			m_downloadLimit,
			m_postJobCount,
			m_threadCount,
			"\033[K");
		if (status.data)
		{
			fwrite(status.data, 1, status.len, stdout);
		}
		nzbget_rs_free(status);
		m_needGoBack = true;
	}

	virtual void PrintSkip()
	{
		printf(".....\033[K\n");
	}

	virtual void BeforeExit()
	{
		if (IsRemoteMode())
		{
			printf("\n");
		}
	}

private:
	bool m_needGoBack = false;
};

#endif
