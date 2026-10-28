Fixed (#1427): Testplan boundary cases now reserve plain names before qualifying
duplicates across actions and guards, then sort cases by their final unique names.
They omit unrepresentable integer boundaries without overflow and index conformance
state values once when constructing shortest setup paths.
