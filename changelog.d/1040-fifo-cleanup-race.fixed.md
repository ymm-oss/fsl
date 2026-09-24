Fixed (#1040): FIFO cleanup joins a finished writer before reading its final outcome, avoiding a race between outcome delivery and the writer's finished state.
