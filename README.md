# linker
A web API that links the bot and the website.

The point of this repository is to provide a way for the website to access the configuration files of the bot. Indeed, since one of the goal of the website is to allow configuration of servers from the web it needs to access the config files.
Thus it was chosen to build this linker. This has many pros, first we can provide a stable API for the website regardless of the changes regarding how the bot handles the config files.
This eases maintenance by keeping the codebase cleaner. Moreover if someone wants to run their own instance of the bot they can stay free of the bloat of the linker.

Initially written in python a rewrite was done in rust to extend its functionnalities, performance and safety. Tests were written along the way. If you wish to improve the linker feel free to do so in any way you see fit. Improvements are very welcome as long as they uphold the current garuantess and code quality. That is to say that all tests should pass and new ones should be amde if you extend the linker.
