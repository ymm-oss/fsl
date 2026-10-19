Changed (#1285): acceptance and forbidden traces reach their consumers only
through the requirement-trace gate, as a type the gate alone can construct; a
consumer that reads them ungated names that at the call site. Command output
and exit codes are unchanged.
