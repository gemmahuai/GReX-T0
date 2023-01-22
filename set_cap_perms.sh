#!/usr/bin/env bash
BINARY=grex_t0
if test -f "target/debug/$BINARY"; then
    sudo setcap cap_net_raw,cap_net_admin+ep target/debug/grex_t0
fi
if test -f "target/release/$BINARY"; then
    sudo setcap cap_net_raw,cap_net_admin+ep target/debug/grex_t0
fi