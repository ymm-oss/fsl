Fixed (#1543): concrete init evaluation failures now name the same containing
statement location as BMC, including assignment targets, if conditions, forall
sources and filters, and inner forall assignments. Explicit and auto no longer
lose the location of an out-of-domain init Map write.
