#ifndef CONFIG_H
#define CONFIG_H

/* Name of package */
#define PACKAGE "rustyget"

/* Version number of package */
#define VERSION "27.0"

/* Define to the address where bug reports for this package should be sent. */
#define PACKAGE_BUGREPORT "https://github.com/xbmc4lyfe/RustyGet/issues"

/* Define to 1 to install an empty signal handler for SIGCHLD */
#define SIGCHLD_HANDLER 1

/* Define to the name of macro which returns the name of function being compiled */
#define FUNCTION_MACRO_NAME __FUNCTION__

#define HAVE_CTIME_R_2 1
#define HAVE_FDATASYNC 1
#define HAVE_FSEEKO 1
#ifdef __APPLE__
#define HAVE_FULLFSYNC 1
#endif
#define HAVE_GETADDRINFO 1
#define HAVE_GETOPT 1
#define HAVE_GETOPT_H 1
#define HAVE_GETOPT_LONG 1
#define HAVE_INTTYPES_H 1
#define HAVE_LOCKF 1
#define HAVE_NCURSES_H 1
#define HAVE_PTHREAD_CANCEL 1
#define HAVE_REGEX_H 1
#define HAVE_SC_NPROCESSORS_ONLN 1
#define HAVE_STDIO_H 1
#define HAVE_STDLIB_H 1
#define HAVE_STRINGS_H 1
#define HAVE_STRING_H 1
#ifdef __linux__
#define HAVE_SYS_PRCTL_H 1
#define HAVE_ENDIAN_H 1
#endif
#define HAVE_SYS_STAT_H 1
#define HAVE_SYS_TYPES_H 1
#define HAVE_UNISTD_H 1
#define HAVE_ALLOCA_H 1
#define HAVE_VARIADIC_MACROS 1
#define HAVE_X509_CHECK_HOST 1
#define SOCKLEN_T socklen_t

#ifndef NZBGET_USE_RUST
#define NZBGET_USE_RUST 1
#endif

#endif /* CONFIG_H */
