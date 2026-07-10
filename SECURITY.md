# Security Policy

Thanks for helping keep Droptime Logger and its users safe.

## Reporting a vulnerability

Please report security issues **privately**. Don't open a public issue, discussion, or pull request for a suspected vulnerability.

Email **ryan@outsidethebox.dev** with:

- what you found and where (a file, a screen, a driver),
- how to reproduce it, and
- what an attacker could actually do with it.

You'll get a reply within **72 hours** acknowledging the report. From there we'll work with you on a fix and a disclosure timeline, and we'll credit you when the fix ships if you'd like the credit.

## Supported versions

Security fixes land on the latest released version. We're pre-1.0, so please make sure you're on the newest release before reporting — you'll find the version in the app menu under **About**.

## What the Logger actually touches

Some context that shapes what counts as a vulnerability here, because the Logger's threat surface is deliberately small:

- **It's local-first.** Your roasts live in a local SQLite database on your machine. The app handles no accounts, no secrets, and no API keys by default.
- **It doesn't phone home.** With one exception, noted below, the Logger makes no network requests. Nothing about your roasts leaves your computer unless you export a file and share it yourself.
- **The one network call is a read-only update check** against our public GitHub Releases (a small version manifest). It sends no telemetry and downloads nothing on its own; if a newer version exists, the app simply tells you.
- **It reads hardware, it never controls it.** The Logger is a roast scope: it reads temperatures over a serial connection and never sends commands that actuate a roaster — no PID, no burner control. That invariant is a safety property, so reports that our code could be coaxed into *writing* to a device are especially welcome.

If you find something that breaks this model — a crafted `.alog` import that does more than fail cleanly, or a path that makes the app write to a serial device — that's exactly the kind of report we want.

## No bug bounty (yet)

We don't run a paid bug-bounty program right now. We deeply value responsible disclosure, we'll work the issue quickly, and we'll credit you publicly when the fix ships.
