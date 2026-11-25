## Configuration

RustyGet needs a configuration file. 

An example configuration file is provided in "rustyget.conf", which
is installed into "`<prefix>`/share/rustyget" (where `<prefix>` depends on
system configuration and configure options - typically "/usr/local",
"/usr" or "/opt"). The installer adjusts the file according to your
system paths. If you have performed the installation step 
"make install-conf" this file is already copied to "`<prefix>`/etc" and
RustyGet finds it automatically. If you install the program manually
from a binary archive you have to copy the file from "`<prefix>`/share/rustyget"
to one of the locations listed below.

Open the file in a text editor and modify it accodring to your needs.

You need to set at least the option "MAINDIR" and one news server in
configuration file. The file has comments on how to use each option.

The program looks for configuration file in following standard 
locations (in this order):

On POSIX systems:
```
<EXE-DIR>/rustyget.conf
~/.rustyget
~/.config/rustyget.conf
~/.config/rustyget/rustyget.conf
/etc/rustyget.conf
/usr/etc/rustyget.conf
/usr/local/etc/rustyget.conf
/opt/etc/rustyget.conf
~/usr/etc/rustyget.conf
```

If none of these exists, an existing `nzbget.conf` is read instead, from
`<EXE-DIR>/nzbget.conf`, `~/.nzbget` and the same folders as above with the
name `nzbget.conf`. The log then says so and names the new path to rename
it to (for example `~/.nzbget` to `~/.rustyget`).

If you put the configuration file in other place, you can use command-
line switch "-c `<filename>`" to point the program to correct location.

In special cases you can run program without configuration file using
switch "-n". You need to use switch "-o" to pass required configuration 
options via command-line.

## Usage

RustyGet can be used in either standalone mode which downloads a single file 
or as a server which is able to queue up numerous download requests.

### Standalone mode:
--------------------
```
rustyget <nzb-file>
```
### Server mode:
----------------

First start the RustyGet server:

  - in console mode:
```
rustyget -s
```
  - or in daemon mode:
```
 rustyget -D
```	
To stop server use:
```
rustyget -Q  
```

When RustyGet is started in console server mode it displays a message that
it is ready to receive download requests. In daemon mode it doesn't print any
messages to console since it runs in background.

When the server is running it is possible to queue up downloads. This can be
done either in terminal with "rustyget -A <nzb-file>" or by uploading
a nzb-file into server's monitor-directory (<MAINDIR>/nzb by default).

To check the status of server start client and connect it to server:
```
rustyget -C
```
The client have three different (display) outputmodes, which you can select
in configuration file (on client computer) or in command line. Try them:
```
rustyget -o outputmode=log -C
rustyget -o outputmode=color -C
rustyget -o outputmode=curses -C
```
To list files in server's queue:
```
rustyget -L
```
It prints something like:
```
[1] nzbname\filename1.rar (50.00 MB)
[2] nzbname\filename1.r01 (50.00 MB)
[3] another-nzb\filename3.r01 (100.00 MB)
[4] another-nzb\filename3.r02 (100.00 MB)
```
This is the list of individual files listed within nzb-file. To print
the list of nzb-files (without content) add G-modifier to the list command:
```
[1] nzbname (4.56 GB)
[2] another-nzb (4.20 GB)
```
The numbers in square braces are ID's of files or groups in queue.
They can be used in edit-command. For example to move file with
ID 2 to the top of queue:
```
rustyget -E T 2
```  
or to pause files with IDs from 10 to 20:
```
rustyget -E P 10-20
```
or to delete files from queue:
```
rustyget -E D 3 10-15 20-21 16
```

The edit-command has also a group-mode which affects all files from the
same nzb-file. You need to pass an ID of the group. For example to delete
the whole group 1:
```
rustyget -E G D 1
```
The switch "o" is useful to override options in configuration files. 
For example:
```
rustyget -o reloadqueue=no -o dupecheck=no -o parcheck=yes -s
```  
or:
```
rustyget -o outputmode=log -C
```

### Running rustyget as systemd service (Linux only)

It is possible to run rustyget as a systemd service under modern Linux distributions.
To do that, create `rustyget.service` file in `/etc/systemd/system/`. Example service files (replace `/path/to/rustyget` with real path, and `rustyget_user`/`rustyget_group` with real user/group):

- rustyget runs as a daemon and logs to own log file:
```
[Unit]
Description=RustyGet Service
After=network.target

[Service]
Type=forking
ExecStart=/path/to/rustyget -D
ExecStop=/path/to/rustyget -Q
User=rustyget_user
Group=rustyget_group
Restart=on-failure

[Install]
WantedBy=multi-user.target
```

- rustyget runs as a server with logging to stdout, logs are forwarded to journald
```
[Unit]
Description=RustyGet Service
After=network.target

[Service]
Type=simple
ExecStart=/path/to/rustyget -s -o OutputMode=log
ExecStop=/path/to/rustyget -Q
User=rustyget_user
Group=rustyget_group
Restart=on-failure

[Install]
WantedBy=multi-user.target
```

Then run (start rustyget and enable autostart):
```
sudo systemctl daemon-reload
sudo systemctl enable rustyget
sudo systemctl start rustyget
```

### Running client & server on seperate machines:
-------------------------------------------------

Since rustyget communicates via TCP/IP it's possible to have a server running on
one computer and adding downloads via a client on another computer.

Do this by setting the "ControlIP" option in the rustyget.conf file to point to the
IP of the server (default is localhost which means client and server runs on 
same computer)

### Security warning
--------------------

RustyGet communicates via unsecured socket connections. This makes it vulnerable.
Although server checks the password passed by client, this password is still 
transmitted in unsecured way. For this reason it is highly recommended 
to configure your Firewall to not expose the port used by RustyGet to WAN. 

If you need to control server from WAN it is better to connect to server's
terminal via SSH and then run
RustyGet client commands in this terminal.

### Web-interface
-----------------

RustyGet has a built-in web-server providing the access to the program
functions via web-interface.

To activate web-interface set the option "WebDir" to the path with
web-interface files. If you install using "make install-conf" as
described above the option is set automatically. If you install using
binary files you should check if the option is set correctly.

To access web-interface from your web-browser use the server address
and port defined in RustyGet configuration file in options "ControlIP" and
"ControlPort". For example:
```
http://localhost:6789/
```

For login credentials type username and the password defined by
options "ControlUsername" (default `"rustyget"`) and "ControlPassword"
(default `"tegbzn6789"`).

In a case your browser forget credentials, to prevent typing them each
time, there is a workaround - use URL in the form:
```
http://localhost:6789/username:password/
```
Please note, that in this case the password is saved in a bookmark or in
browser history in plain text and is easy to find by persons having
access to your computer.

## RSS and duplicate check


### Coming soon
