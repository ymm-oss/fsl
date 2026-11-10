Fixed (#1808): in an FSL 5 `requirements` document, a second relation made by
the same event (`Order belongs to User as buyer via place` and `Order belongs
to User as payer via place`, or `所属先 … 役名 … 契機 place` twice) is now
refused at the second relation with its own code, `event_already_makes_relation`,
and a message naming the first relation ("event 'place' already makes the
relation 'buyer'; an event can make one relation, so give 'payer' its own
event"). It was refused at the event as `generated_name_collision`, telling the
author to rename an input the document does not write. The document is still
refused (`kind: "parse"`, exit 2). An authored input named like the generated
one (`with u: Qty`) keeps `generated_name_collision` and its "rename the input"
message. The limit is now stated in both language references and the
fsl-requirements and fsl-verify skill references.
