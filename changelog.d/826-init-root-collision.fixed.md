Fixed (#826): unresolved indexed `init` writes now conservatively collide with every concrete key on the same logical map root instead of silently accepting aliasing writes with equal values.
