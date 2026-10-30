Use one core declaration merge for refinement typechecking, concrete evaluation
and preserved-progress verification. Pulled-model safety violations now fail
closed with a non-solver semantics error (exit 2), naming the property and pulled
model without a trace, instead of being discarded as successful refinement.
