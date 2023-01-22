#!/usr/bin/env bash
sudo setcap cap_net_raw,cap_net_admin+ep target/debug/grex_t0
sudo setcap cap_net_raw,cap_net_admin+ep target/release/grex_t0