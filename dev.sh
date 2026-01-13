#!/bin/bash
# Hirsel development script
# Force X11 backend to work around Wayland/WebKitGTK click offset bug
GDK_BACKEND=x11 npm run dev
