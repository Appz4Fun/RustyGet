/*
 *  This file is part of nzbget. See <https://nzbget.com>.
 *
 *  Copyright (C) 2004 Sven Henkel <sidddy@users.sourceforge.net>
 *  Copyright (C) 2007-2019 Andrey Prygunkov <hugbug@users.sourceforge.net>
 *  Copyright (C) 2024-2025 Denis <denis@nzbget.com>
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

#ifndef DISABLE_CURSES

// "ncurses.h" contains many global defines such as for "OK" or "clear" which we sure don't want
// everywhere in the project. For that reason we include "ncurses.h" directly here instead of
// putting it into global header file "nzbget.h".

#ifdef HAVE_NCURSES_H
#include <ncurses.h>
#endif
#ifdef HAVE_NCURSES_NCURSES_H
#include <ncurses/ncurses.h>
#endif

#include "NCursesFrontend.h"
#include "Options.h"
#include "Util.h"
#include "nzbget_rs.h"

// curses.h on Solaris declares "clear()" via DEFINE. That causes problems, because
// it also affects calls to deque's method "clear()", producing compiler errors.
// We use function "curses_clear()" to call macro "clear" of curses, then
// undefine macro "clear".
void curses_clear()
{
	clear();
}
#undef clear

extern void ExitProc();

const int NCURSES_COLORPAIR_TEXT		= 1;
const int NCURSES_COLORPAIR_INFO		= 2;
const int NCURSES_COLORPAIR_WARNING		= 3;
const int NCURSES_COLORPAIR_ERROR		= 4;
const int NCURSES_COLORPAIR_DEBUG		= 5;
const int NCURSES_COLORPAIR_DETAIL		= 6;
const int NCURSES_COLORPAIR_STATUS		= 7;
const int NCURSES_COLORPAIR_KEYBAR		= 8;
const int NCURSES_COLORPAIR_INFOLINE	= 9;
const int NCURSES_COLORPAIR_TEXTHIGHL 	= 10;
const int NCURSES_COLORPAIR_CURSOR		= 11;
const int NCURSES_COLORPAIR_HINT		= 12;

const int MAX_SCREEN_WIDTH				= 512;

static const int READKEY_EMPTY = ERR;

NCursesFrontend::NCursesFrontend()
	: m_startHint(0)
	, m_colWidthFiles(0)
	, m_colWidthTotal(0)
	, m_colWidthLeft(0)
	, m_inputValue(0)
{
	m_summary = true;
	m_fileList = true;

	m_showNzbname = g_Options->GetCursesNzbName();
	m_showTimestamp = g_Options->GetCursesTime();
	m_groupFiles = g_Options->GetCursesGroup();

	// Setup curses
	m_window = initscr();
	if (m_window == nullptr)
	{
		printf("ERROR: m_pWindow == nullptr\n");
		exit(-1);
	}
	keypad(stdscr, true);
	nodelay((WINDOW*)m_window, true);
	noecho();
	curs_set(0);
	m_useColor = has_colors();

	if (m_useColor)
	{
		start_color();
		init_pair(0,							COLOR_WHITE,	COLOR_BLUE);
		init_pair(NCURSES_COLORPAIR_TEXT,		COLOR_WHITE,	COLOR_BLACK);
		init_pair(NCURSES_COLORPAIR_INFO,		COLOR_GREEN,	COLOR_BLACK);
		init_pair(NCURSES_COLORPAIR_WARNING,	COLOR_MAGENTA,	COLOR_BLACK);
		init_pair(NCURSES_COLORPAIR_ERROR,		COLOR_RED,		COLOR_BLACK);
		init_pair(NCURSES_COLORPAIR_DEBUG,		COLOR_WHITE,	COLOR_BLACK);
		init_pair(NCURSES_COLORPAIR_DETAIL,		COLOR_GREEN,	COLOR_BLACK);
		init_pair(NCURSES_COLORPAIR_STATUS,		COLOR_BLUE,		COLOR_WHITE);
		init_pair(NCURSES_COLORPAIR_KEYBAR,		COLOR_WHITE,	COLOR_BLUE);
		init_pair(NCURSES_COLORPAIR_INFOLINE,	COLOR_WHITE,	COLOR_BLUE);
		init_pair(NCURSES_COLORPAIR_TEXTHIGHL,	COLOR_BLACK,	COLOR_CYAN);
		init_pair(NCURSES_COLORPAIR_CURSOR,		COLOR_BLACK,	COLOR_YELLOW);
		init_pair(NCURSES_COLORPAIR_HINT,		COLOR_WHITE,	COLOR_RED);
	}
}

NCursesFrontend::~NCursesFrontend()
{
	keypad(stdscr, false);
	echo();
	curs_set(1);
	endwin();
	printf("\n");
	SetHint(nullptr);
}

void NCursesFrontend::Run()
{
	debug("Entering NCursesFrontend-loop");

	m_dataUpdatePos = 0;

	while (!IsStopped())
	{
		// The data (queue and log) is updated each m_iUpdateInterval msec,
		// but the window is updated more often for better reaction on user's input

		bool updateNow = false;
		int key = ReadConsoleKey();

		int updateNext = m_updateNextTime ? 1 : 0;
		updateNow = nzbget_rs_sfw_ncurses_decide_loop_update(
			key != READKEY_EMPTY ? 1 : 0, &updateNext, m_dataUpdatePos) != 0;
		m_updateNextTime = updateNext != 0;

		if (updateNow)
		{
			Update(key);
		}

		if (m_dataUpdatePos <= 0)
		{
			m_dataUpdatePos = m_updateInterval;
		}

		// update more often (sleep shorter) if need faster reaction on user input
		int sleepInterval = nzbget_rs_sfw_ncurses_select_sleep_interval(m_inputMode == normal ? 1 : 0);
		Wait(sleepInterval);
		m_dataUpdatePos -= sleepInterval;
	}

	FreeData();

	debug("Exiting NCursesFrontend-loop");
}

void NCursesFrontend::NeedUpdateData()
{
	m_dataUpdatePos = 10;
	m_updateNextTime = true;
}

void NCursesFrontend::Update(int key)
{
	// Figure out how big the screen is
	CalcWindowSizes();

	if (m_dataUpdatePos <= 0)
	{
		FreeData();
		m_neededLogEntries = m_messagesWinClientHeight;
		if (!PrepareData())
		{
			return;
		}

		// recalculate frame sizes
		CalcWindowSizes();
	}

	if (m_inputMode == editQueue)
	{
		int queueSize = CalcQueueSize();
		if (queueSize == 0)
		{
			m_selectedQueueEntry = 0;
			m_inputMode = normal;
		}
	}

	//------------------------------------------
	// Print Current NZBInfoList
	//------------------------------------------
	if (m_queueWinHeight > 0)
	{
		PrintQueue();
	}

	//------------------------------------------
	// Print Messages
	//------------------------------------------
	if (m_messagesWinHeight > 0)
	{
		PrintMessages();
	}

	PrintStatus();

	PrintKeyInputBar();

	UpdateInput(key);

	RefreshScreen();
}

void NCursesFrontend::CalcWindowSizes()
{
	int nrRows, nrColumns;
	getmaxyx(stdscr, nrRows, nrColumns);
	// the line buffers hold MAX_SCREEN_WIDTH columns (a wider terminal wrote past
	// them), and messages wrap at the width less 8 (a divisor of zero at 8)
	int queueSize = CalcQueueSize();
	int qh = 0, qch = 0, mtop = 0, mh = 0, mch = 0;
	nzbget_rs_feh_ncurses_calc_window_sizes(
		nrRows, nrColumns, MAX_SCREEN_WIDTH, m_queueWindowPercentage, queueSize,
		&nrColumns, &qh, &qch, &mtop, &mh, &mch);
	if (nrRows != m_screenHeight || nrColumns != m_screenWidth)
	{
		curses_clear();
		m_screenHeight = nrRows;
		m_screenWidth = nrColumns;
	}
	m_queueWinTop = 0;
	m_queueWinHeight = qh;
	m_queueWinClientHeight = qch;
	m_messagesWinTop = mtop;
	m_messagesWinHeight = mh;
	m_messagesWinClientHeight = mch;
}

int NCursesFrontend::CalcQueueSize()
{
	int queueSize = 0;
	if (m_groupFiles)
	{
		queueSize = DownloadQueue::Guard()->GetQueue()->size();
	}
	else
	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
		for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
		{
			queueSize += nzbInfo->GetFileList()->size();
		}
	}
	return queueSize;
}

void NCursesFrontend::PlotLine(const char* str, int row, int pos, int colorPair)
{
	size_t size = Util::SafeIntCast<int, size_t>(m_screenWidth + 1);
	std::string buffer(size, '\0');
	snprintf(buffer.data(), size, "%-*s", m_screenWidth, str);

	if (Util::CmpGreater(size, m_screenWidth - pos) && m_screenWidth - pos < MAX_SCREEN_WIDTH)
	{
		size = Util::SafeIntCast<int, size_t>(m_screenWidth - pos);
		buffer.resize(size);
	}

	PlotText(buffer.c_str(), row, pos, colorPair, false);
}

void NCursesFrontend::PlotText(const char * string, int row, int pos, int colorPair, bool blink)
{
	if( m_useColor )
	{
		attron(COLOR_PAIR(colorPair));
		if (blink)
		{
			attron(A_BLINK);
		}
	}
	mvaddstr(row, pos, (char*)string);
	if( m_useColor )
	{
		attroff(COLOR_PAIR(colorPair));
		if (blink)
		{
			attroff(A_BLINK);
		}
	}
}

void NCursesFrontend::RefreshScreen()
{
	// Cursor placement
	wmove((WINDOW*)m_window, m_screenHeight, m_screenWidth);

	// NCurses refresh
	refresh();
}

void NCursesFrontend::PrintMessages()
{
	int lineNr = m_messagesWinTop;

	PlotLine(
		nzbget_rs_tpr_ncurses_format_messages_header(m_useColor ? 1 : 0),
		lineNr++, 0, NCURSES_COLORPAIR_INFOLINE);

	int line = lineNr + m_messagesWinClientHeight - 1;
	int linesToPrint = m_messagesWinClientHeight;

	GuardedMessageList messages = GuardMessages();

	// print messages from bottom
	for (int i = (int)messages->size() - 1; i >= 0 && linesToPrint > 0; i--)
	{
		int printedLines = PrintMessage(messages->at(i), line, linesToPrint);
		line -= printedLines;
		linesToPrint -= printedLines;
	}

	if (linesToPrint > 0)
	{
		// too few messages, print them again from top
		line = lineNr + m_messagesWinClientHeight - 1;
		while (linesToPrint-- > 0)
		{
			PlotLine("", line--, 0, NCURSES_COLORPAIR_TEXT);
		}
		int linesToPrint2 = m_messagesWinClientHeight;
		for (int i = (int)messages->size() - 1; i >= 0 && linesToPrint2 > 0; i--)
		{
			int printedLines = PrintMessage(messages->at(i), line, linesToPrint2);
			line -= printedLines;
			linesToPrint2 -= printedLines;
		}
	}
}

int NCursesFrontend::PrintMessage(Message& msg, int row, int maxLines)
{
	CString text;

	if (m_showTimestamp)
	{
		time_t rawtime = msg.GetTime() + g_Options->GetTimeCorrection();
		NzbgetRsBuf tsBuf = nzbget_rs_tpr_ncurses_format_timestamp_message(
			*Util::FormatTime(rawtime), msg.GetText());
		text = tsBuf.data ? tsBuf.data : "";
		nzbget_rs_free(tsBuf);
	}
	else
	{
		text = msg.GetText();
	}

	// replace some special characters with spaces
	int len = (int)strlen(text);
	nzbget_rs_ccm_ncurses_sanitize_message_chars((char*)text, (size_t)len);
	int winWidth = 0, msgLines = 0;
	nzbget_rs_ccm_ncurses_calc_msg_lines(len, m_screenWidth, &winWidth, &msgLines);

	int lines = 0;
	for (int i = msgLines - 1; i >= 0 && lines < maxLines; i--)
	{
		int r = row - msgLines + i + 1;
		PlotLine(text + winWidth * i, r, 8, NCURSES_COLORPAIR_TEXT);
		if (i == 0)
		{
			PlotText(m_messageTypes[msg.GetKind()], r, 0, m_messageColorTypes[msg.GetKind()], false);
		}
		else
		{
			PlotText("        ", r, 0, m_messageColorTypes[msg.GetKind()], false);
		}
		lines++;
	}

	return lines;
}

void NCursesFrontend::PrintStatus()
{
	int statusRow = m_screenHeight - 2;

	NzbgetRsNcursesStatusSpec spec{};
	spec.stand_by = m_standBy ? 1 : 0;
	spec.pause_download = m_pauseDownload ? 1 : 0;
	spec.current_download_speed = m_currentDownloadSpeed;
	spec.remaining_size = m_remainingSize;
	spec.download_limit = m_downloadLimit;
	spec.post_job_count = m_postJobCount;
	spec.dn_time_sec = m_dnTimeSec;
	spec.all_bytes = m_allBytes;
	spec.thread_count = m_threadCount;

	NzbgetRsBuf buf = nzbget_rs_ncurses_format_status_bar(&spec);
	if (buf.data)
	{
		PlotLine(buf.data, statusRow, 0, NCURSES_COLORPAIR_STATUS);
	}
	nzbget_rs_free(buf);
}

void NCursesFrontend::PrintKeyInputBar()
{
	int queueSize = CalcQueueSize();
	int inputBarRow = m_screenHeight - 1;

	if (!m_hint.empty())
	{
		time_t time = Util::CurrentTime();
		if (nzbget_rs_sfw_ncurses_is_hint_active((int64_t)time, (int64_t)m_startHint) != 0)
		{
			PlotLine(m_hint.c_str(), inputBarRow, 0, NCURSES_COLORPAIR_HINT);
			return;
		}
		else
		{
			SetHint(nullptr);
		}
	}

	switch (m_inputMode)
	{
	case normal:
		PlotLine(
			nzbget_rs_sfw_ncurses_select_normal_keybar(m_groupFiles ? 1 : 0),
			inputBarRow, 0, NCURSES_COLORPAIR_KEYBAR);
		break;
	case editQueue:
	{
		const char* status = nzbget_rs_ccm_ncurses_select_edit_queue_keybar(
			m_selectedQueueEntry, queueSize);

		PlotLine(status, inputBarRow, 0, NCURSES_COLORPAIR_KEYBAR);
		break;
	}
	case downloadRate:
	{
		NzbgetRsBuf hintBuf = nzbget_rs_tpr_ncurses_format_download_rate_hint(m_inputValue);
		PlotLine(hintBuf.data ? hintBuf.data : "", inputBarRow, 0, NCURSES_COLORPAIR_KEYBAR);
		nzbget_rs_free(hintBuf);

		// Print the cursor
		PlotText(" ", inputBarRow, 15 + m_inputNumberIndex, NCURSES_COLORPAIR_CURSOR, true);
		break;
	}
	}
}

void NCursesFrontend::SetHint(const char* hint)
{
	if (hint) m_hint = hint;
	if (!m_hint.empty()) m_startHint = Util::CurrentTime();
}

void NCursesFrontend::PrintQueue()
{
	if (m_groupFiles)
	{
		PrintGroupQueue();
	}
	else
	{
		PrintFileQueue();
	}
}

void NCursesFrontend::PrintFileQueue()
{
	int lineNr = m_queueWinTop + 1;
	int64 remaining = 0;
	int64 paused = 0;
	int pausedFiles = 0;
	int fileNum = 0;

	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
	for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
	{
		for (FileInfo* fileInfo : nzbInfo->GetFileList())
		{
			if (fileNum >= m_queueScrollOffset && fileNum < m_queueScrollOffset + m_queueWinHeight -1)
			{
				PrintFilename(fileInfo, lineNr++, fileNum == m_selectedQueueEntry);
			}
			fileNum++;

			if (fileInfo->GetPaused())
			{
				pausedFiles++;
				paused += fileInfo->GetRemainingSize();
			}
			remaining += fileInfo->GetRemainingSize();
		}
	}

	NzbgetRsBuf headerBuf = nzbget_rs_ncurses_format_file_queue_header(
		m_useColor ? 1 : 0, fileNum, pausedFiles, remaining, paused);
	if (fileNum > 0)
	{
		if (headerBuf.data)
		{
			PrintTopHeader(headerBuf.data, m_queueWinTop, true);
		}
		nzbget_rs_free(headerBuf);
	}
	else
	{
		lineNr--;
		if (headerBuf.data)
		{
			PrintTopHeader(headerBuf.data, lineNr++, true);
		}
		nzbget_rs_free(headerBuf);
		PlotLine("Ready to receive nzb-job", lineNr++, 0, NCURSES_COLORPAIR_TEXT);
	}
}

void NCursesFrontend::PrintFilename(FileInfo * fileInfo, int row, bool selected)
{
	NzbgetRsNcursesFileRowSpec spec{};
	spec.edit_queue_mode = m_inputMode == editQueue ? 1 : 0;
	spec.selected = selected ? 1 : 0;
	spec.use_color = m_useColor ? 1 : 0;
	spec.color_highl = NCURSES_COLORPAIR_TEXTHIGHL;
	spec.color_text = NCURSES_COLORPAIR_TEXT;
	spec.id = fileInfo->GetId();
	spec.priority = fileInfo->GetNzbInfo()->GetPriority();
	spec.active_downloads = fileInfo->GetActiveDownloads();
	spec.paused = fileInfo->GetPaused() ? 1 : 0;
	spec.show_nzbname = m_showNzbname ? 1 : 0;
	spec.path_sep = (uint8_t)PATH_SEPARATOR;
	spec.nzb_name = fileInfo->GetNzbInfo()->GetName();
	spec.filename = fileInfo->GetFilename();
	spec.size = fileInfo->GetSize();
	spec.remaining_size = fileInfo->GetRemainingSize();

	int color = NCURSES_COLORPAIR_TEXT;
	NzbgetRsBuf buf = nzbget_rs_ncurses_format_file_row(&spec, &color);
	if (buf.data)
	{
		PlotLine(buf.data, row, 0, color);
	}
	nzbget_rs_free(buf);
}

void NCursesFrontend::PrintTopHeader(char* header, int lineNr, bool upTime)
{
	int time = upTime ? m_upTimeSec : m_dnTimeSec;
	NzbgetRsBuf buf = nzbget_rs_ncurses_format_top_header(
		header, m_screenWidth, upTime ? 1 : 0, time);
	if (buf.data)
	{
		PlotLine(buf.data, lineNr, 0, NCURSES_COLORPAIR_INFOLINE);
	}
	nzbget_rs_free(buf);
}

void NCursesFrontend::PrintGroupQueue()
{
	GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();

	int lineNr = m_queueWinTop;

	if (downloadQueue->GetQueue()->empty())
	{
		NzbgetRsBuf headerBuf = nzbget_rs_ncurses_format_group_queue_header(
			m_useColor ? 1 : 0, 0, 0, 0);
		if (headerBuf.data)
		{
			PrintTopHeader(headerBuf.data, lineNr++, false);
		}
		nzbget_rs_free(headerBuf);
		PlotLine("Ready to receive nzb-job", lineNr++, 0, NCURSES_COLORPAIR_TEXT);
	}
	else
	{
		lineNr++;

		ResetColWidths();
		int calcLineNr = lineNr;
		int i = 0;
		for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
		{
			if (i >= m_queueScrollOffset && i < m_queueScrollOffset + m_queueWinHeight -1)
			{
				PrintGroupname(nzbInfo, calcLineNr++, false, true);
			}
			i++;
		}

		int64 remaining = 0;
		int64 paused = 0;
		i = 0;
		for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
		{
			if (i >= m_queueScrollOffset && i < m_queueScrollOffset + m_queueWinHeight -1)
			{
				PrintGroupname(nzbInfo, lineNr++, i == m_selectedQueueEntry, false);
			}
			i++;
			remaining += nzbInfo->GetRemainingSize();
			paused += nzbInfo->GetPausedSize();
		}

		NzbgetRsBuf headerBuf = nzbget_rs_ncurses_format_group_queue_header(
			m_useColor ? 1 : 0, (int)downloadQueue->GetQueue()->size(), remaining, paused);
		if (headerBuf.data)
		{
			PrintTopHeader(headerBuf.data, m_queueWinTop, false);
		}
		nzbget_rs_free(headerBuf);
	}
}

void NCursesFrontend::ResetColWidths()
{
	m_colWidthFiles = 0;
	m_colWidthTotal = 0;
	m_colWidthLeft = 0;
}

void NCursesFrontend::PrintGroupname(NzbInfo* nzbInfo, int row, bool selected, bool calcColWidth)
{
	NzbgetRsNcursesGroupRowSpec spec{};
	spec.edit_queue_mode = m_inputMode == editQueue ? 1 : 0;
	spec.selected = selected ? 1 : 0;
	spec.use_color = m_useColor ? 1 : 0;
	spec.color_highl = NCURSES_COLORPAIR_TEXTHIGHL;
	spec.color_text = NCURSES_COLORPAIR_TEXT;
	spec.screen_width = m_screenWidth;
	spec.calc_col_width = calcColWidth ? 1 : 0;
	spec.id = nzbInfo->GetId();
	spec.priority = nzbInfo->GetPriority();
	spec.active_downloads = nzbInfo->GetActiveDownloads();
	spec.name = nzbInfo->GetName();
	spec.file_count = (int)nzbInfo->GetFileList()->size();
	spec.paused_file_count = nzbInfo->GetPausedFileCount();
	spec.size = nzbInfo->GetSize();
	spec.remaining_size = nzbInfo->GetRemainingSize();
	spec.paused_size = nzbInfo->GetPausedSize();
	spec.stand_by = m_standBy ? 1 : 0;
	spec.pause_download = m_pauseDownload ? 1 : 0;
	spec.current_download_speed = m_currentDownloadSpeed;

	int color = NCURSES_COLORPAIR_TEXT;
	NzbgetRsBuf buf = nzbget_rs_ncurses_format_group_row(
		&spec, &m_colWidthFiles, &m_colWidthTotal, &m_colWidthLeft, &color);
	if (!calcColWidth && buf.data)
	{
		PlotLine(buf.data, row, 0, color);
	}
	nzbget_rs_free(buf);
}

bool NCursesFrontend::EditQueue(DownloadQueue::EEditAction action, int offset)
{
	int ID = 0;

	if (m_groupFiles)
	{
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
		if (m_selectedQueueEntry >= 0 && m_selectedQueueEntry < (int)downloadQueue->GetQueue()->size())
		{
			std::unique_ptr<NzbInfo>& nzbInfo = downloadQueue->GetQueue()->at(m_selectedQueueEntry);
			ID = nzbInfo->GetId();
			int newLastPausePars = m_lastPausePars ? 1 : 0;
			action = (DownloadQueue::EEditAction)nzbget_rs_feh_ncurses_resolve_group_pause_action(
				(int)action, nzbInfo->GetRemainingSize(), nzbInfo->GetPausedSize(),
				nzbInfo->GetRemainingParCount(), m_lastPausePars ? 1 : 0,
				m_lastEditEntry, m_selectedQueueEntry, &newLastPausePars);
			m_lastPausePars = newLastPausePars != 0;
		}

		// map file-edit-actions to group-edit-actions
		action = (DownloadQueue::EEditAction)nzbget_rs_tpr_ncurses_map_file_to_group_action((int)action);
	}
	else
	{
		int fileNum = 0;
		GuardedDownloadQueue downloadQueue = DownloadQueue::Guard();
		for (NzbInfo* nzbInfo : downloadQueue->GetQueue())
		{
			for (FileInfo* fileInfo : nzbInfo->GetFileList())
			{
				if (m_selectedQueueEntry == fileNum)
				{
					ID = fileInfo->GetId();
					action = (DownloadQueue::EEditAction)nzbget_rs_tpr_ncurses_resolve_file_pause_action(
						(int)action, (int)DownloadQueue::eaFilePause,
						(int)DownloadQueue::eaFileResume, fileInfo->GetPaused() ? 1 : 0);
				}
				fileNum++;
			}
		}
	}

	m_lastEditEntry = m_selectedQueueEntry;

	NeedUpdateData();

	if (ID != 0)
	{
		return ServerEditQueue(action, offset, ID);
	}
	else
	{
		return false;
	}
}

void NCursesFrontend::SetCurrentQueueEntry(int entry)
{
	int queueSize = CalcQueueSize();

	nzbget_rs_feh_ncurses_calc_scroll_and_selection(
		entry, queueSize, m_queueScrollOffset, m_queueWinClientHeight,
		&m_selectedQueueEntry, &m_queueScrollOffset);
}


/*
 * Process keystrokes starting with the initialKey, which must not be
 * READKEY_EMPTY but has alread been set via ReadConsoleKey.
 */
void NCursesFrontend::UpdateInput(int initialKey)
{
	int key = initialKey;
	while (key != READKEY_EMPTY)
	{
		int queueSize = CalcQueueSize();

		// Normal or edit queue mode
		if (m_inputMode == normal || m_inputMode == editQueue)
		{
			switch (key)
			{
			case 'q':
				// Key 'q' for quit
				ExitProc();
				break;
			case 'z':
				// show/hide NZBFilename
				m_showNzbname = !m_showNzbname;
				break;
			case 'w':
				// swicth window sizes
				m_queueWindowPercentage = nzbget_rs_ccm_ncurses_next_queue_win_pct(
					m_queueWindowPercentage, m_inputMode == editQueue ? 1 : 0);
				CalcWindowSizes();
				SetCurrentQueueEntry(m_selectedQueueEntry);
				break;
			case 'g':
				// group/ungroup files
				m_groupFiles = !m_groupFiles;
				SetCurrentQueueEntry(m_selectedQueueEntry);
				NeedUpdateData();
				break;
			}
		}

		// Normal mode
		if (m_inputMode == normal)
		{
			switch (key)
			{
			case 'p':
				// Key 'p' for pause
				if (!IsRemoteMode())
				{
					info("%s", nzbget_rs_tpr_ncurses_pause_log_msg(m_pauseDownload ? 1 : 0));
				}
				ServerPauseUnpause(!m_pauseDownload);
				break;
			case 'e':
			case 10: // return
			case 13: // enter
				if (queueSize > 0)
				{
					m_inputMode = editQueue;
					if (m_queueWindowPercentage == 0)
					{
						m_queueWindowPercentage = 50;
					}
					return;
				}
				break;
			case 'r':
				// Download rate
				m_inputMode = downloadRate;
				m_inputNumberIndex = 0;
				m_inputValue = 0;
				return;
			case 't':
				// show/hide Timestamps
				m_showTimestamp = !m_showTimestamp;
				break;
			}
		}

		// Edit Queue mode
		if (m_inputMode == editQueue)
		{
			switch (key)
			{
			case 'e':
			case 10: // return
			case 13: // enter
				m_inputMode = normal;
				return;
			case KEY_DOWN:
				if (m_selectedQueueEntry < queueSize - 1)
				{
					SetCurrentQueueEntry(m_selectedQueueEntry + 1);
				}
				break;
			case KEY_UP:
				if (m_selectedQueueEntry > 0)
				{
					SetCurrentQueueEntry(m_selectedQueueEntry - 1);
				}
				break;
			case KEY_PPAGE:
			case KEY_NPAGE:
			{
				int newScroll = m_queueScrollOffset;
				int newEntry = m_selectedQueueEntry;
				if (nzbget_rs_tpr_ncurses_calc_page_nav(
					key == KEY_NPAGE ? 1 : 0, m_selectedQueueEntry, queueSize,
					m_queueScrollOffset, m_queueWinClientHeight, &newScroll, &newEntry))
				{
					m_queueScrollOffset = newScroll;
					SetCurrentQueueEntry(newEntry);
				}
				break;
			}
			case KEY_HOME:
				SetCurrentQueueEntry(0);
				break;
			case KEY_END:
				SetCurrentQueueEntry(queueSize > 0 ? queueSize - 1 : 0);
				break;
			case 'p':
				// Key 'p' for pause
				EditQueue(DownloadQueue::eaFilePause, 0);
				break;
			case 'd':
				SetHint(" Use Uppercase \"D\" for delete");
				break;
			case 'D':
				// Delete entry
				if (EditQueue(DownloadQueue::eaFileDelete, 0))
				{
					SetCurrentQueueEntry(m_selectedQueueEntry);
				}
				break;
			case 'u':
				if (EditQueue(DownloadQueue::eaFileMoveOffset, -1))
				{
					SetCurrentQueueEntry(m_selectedQueueEntry - 1);
				}
				break;
			case 'n':
				if (EditQueue(DownloadQueue::eaFileMoveOffset, +1))
				{
					SetCurrentQueueEntry(m_selectedQueueEntry + 1);
				}
				break;
			case 't':
				if (EditQueue(DownloadQueue::eaFileMoveTop, 0))
				{
					SetCurrentQueueEntry(0);
				}
				break;
			case 'b':
				if (EditQueue(DownloadQueue::eaFileMoveBottom, 0))
				{
					SetCurrentQueueEntry(queueSize > 0 ? queueSize - 1 : 0);
				}
				break;
			}
		}

		// Edit download rate input mode
		if (m_inputMode == downloadRate)
		{
			int rateStep = nzbget_rs_feh_ncurses_step_rate_input(
				key, KEY_BACKSPACE, &m_inputNumberIndex, &m_inputValue);
			if (rateStep == 1)
			{
				ServerSetDownloadRate(nzbget_rs_tpr_webdl_kb_to_bytes(m_inputValue));
				m_inputMode = normal;
				return;
			}
			else if (rateStep == 2)
			{
				m_inputMode = normal;
				return;
			}
		}

		key = ReadConsoleKey();
	}
}

int NCursesFrontend::ReadConsoleKey()
{
	return getch();
}

#endif
