rustyget (unreleased)
  - Important notes (upgrading):
    - Without a real password (ControlPassword empty, the stock "tegbzn6789",
      or an address let in by AuthorizedIP), an API call made by a web
      browser (it sends Origin, Referer or Sec-Fetch-* headers) must name this
      computer's address, localhost, ControlIP or a name in the new option
      ControlHostnames (in Host and in X-Forwarded-Host / Forwarded), and must
      show that it comes from RustyGet's own page. Other web sites can no
      longer control RustyGet through your browser, including by DNS
      rebinding. API tools that send none of those headers, such as Sonarr or
      Radarr calling http://rustyget:6789/jsonrpc, work exactly as before.
    - If you open RustyGet in a browser without a password through a host
      name or a reverse proxy (for example nginx's default proxy_pass, which
      sends Host: 127.0.0.1 while the browser is on nzb.example.com), the page
      may now answer 403 and says why: set a password (Settings > Security),
      or add the name to ControlHostnames. A proxy that rewrites Host without
      X-Forwarded-Host can't be told apart from another site behind the same
      proxy, so RustyGet behind a reverse proxy should always have a password.
      X-Forwarded-Host and Forwarded can only make a call stricter (a page
      can set them itself); behind a proxy, list the browser's host name in
      ControlHostnames. Refusals are logged at most once per 10 seconds, with
      a count of the ones suppressed.
    - Known limits without a password: an old browser that sends no
      Sec-Fetch-* headers, or a browser set to send no Referer, can't always
      be told apart from RustyGet's own page. Set a password to be fully
      protected.
    - With no configuration file, "rustyget -s" / "-D" writes
      ~/.config/rustyget/rustyget.conf with a random password, printed once on
      the console (in a container: in docker logs; there it listens on all
      interfaces). "-c <missing file>" creates that file with a warning.
    - A configuration file that exists but can't be read stops RustyGet, as
      before; a file with broken content starts on the defaults for the bad
      lines, with one warning per line.
    - Settings checks every value before saving, backs the file up
      (rustyget.conf.bak-<date>-<time>, the newest 10 and the first kept) and
      asks before removing lines that are not settings.
