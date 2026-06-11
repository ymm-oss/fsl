"""FSL grammar: Lark grammar string + AST transformer + parser instance."""
from lark import Lark, Transformer, v_args

# ---------------------------------------------------------------- grammar

GRAMMAR = r"""
start: "spec" NAME "{" item* "}"

?item: const_def | type_def | enum_def | struct_def
     | state_def | init_def | action_def
     | invariant_def | reachable_def

const_def: "const" NAME "=" expr
type_def: "type" NAME "=" expr ".." expr
enum_def: "enum" NAME "{" enum_member ("," enum_member)* ","? "}"
enum_member: NAME -> enum_member
struct_def: "struct" NAME "{" field ("," field)* ","? "}"
field: NAME ":" type

state_def: "state" "{" var_decl ("," var_decl)* ","? "}"
var_decl: NAME ":" type
?type: "Int"  -> t_int
     | "Bool" -> t_bool
     | "Map" "<" type "," type ">" -> t_map
     | "Set" "<" type ">" -> t_set
     | "Option" "<" type ">" -> t_option
     | NAME -> t_name

init_def: "init" "{" stmt* "}"

action_def: "action" NAME "(" [param ("," param)*] ")" "{" action_item* "}"
param: NAME ":" NAME -> param_typed
     | NAME "in" expr ".." expr -> param_range
?action_item: requires_clause | ensures_clause | let_clause | stmt
requires_clause: "requires" expr
ensures_clause: "ensures" expr
let_clause: "let" NAME "=" expr

?stmt: assign | if_stmt | forall_stmt
assign: lvalue "=" expr
if_stmt: "if" expr "{" stmt_list "}" else_opt?
else_opt: "else" "{" stmt_list "}"
stmt_list: stmt*
forall_stmt: "forall" binder [":"] "{" stmt_list "}"
lvalue: NAME "[" expr "]" "." NAME -> lvalue_map_field
      | NAME "." NAME -> lvalue_field
      | NAME "[" expr "]" -> lvalue_index
      | NAME -> lvalue_var

binder: NAME ":" NAME ["where" expr] -> binder_typed
       | NAME "in" expr ".." expr -> binder_range

invariant_def: "invariant" NAME "{" expr "}"
reachable_def: "reachable" NAME "{" expr "}"

?expr: quant | implies
quant: "forall" binder [":"] expr -> quant_forall
     | "forall" binder [":"] "{" expr "}" -> quant_forall_brace
     | "exists" binder [":"] expr -> quant_exists
     | "exists" binder [":"] "{" expr "}" -> quant_exists_brace
?implies: or_e | or_e "=>" implies -> imp
?or_e: and_e | or_e "or" and_e -> or_op
?and_e: not_e | and_e "and" not_e -> and_op
?not_e: "not" not_e -> not_op
       | is_e
?is_e: cmp ["is" pattern] -> is_pat
pattern: "none" -> pat_none
       | "some" "(" NAME ")" -> pat_some
?cmp: sum | sum CMPOP sum -> cmp_op
?sum: product | sum "+" product -> add | sum "-" product -> sub
?product: unary | product "*" unary -> mul
?unary: "-" unary -> neg | postfix
postfix: atom postfix_suffix*
postfix_suffix: "[" expr "]" -> idx_suffix
               | "." "contains" "(" [expr_list] ")" -> method_contains
               | "." "add" "(" [expr_list] ")" -> method_add
               | "." "remove" "(" [expr_list] ")" -> method_remove
               | "." "size" "(" ")" -> method_size
               | "." NAME -> field_suffix
?atom: INT -> num
     | "true" -> true_lit
     | "false" -> false_lit
     | "none" -> none_lit
     | "some" "(" expr ")" -> some_lit
     | "Set" "{" [expr_list] "}" -> set_lit
     | NAME struct_fields -> struct_lit
     | "count" "(" NAME ":" NAME "where" expr ")" -> count_e
     | "sum" "(" NAME ":" NAME "of" expr ["where" expr] ")" -> sum_e
     | "min" "(" expr "," expr ")" -> min_e
     | "max" "(" expr "," expr ")" -> max_e
     | "abs" "(" expr ")" -> abs_e
     | "old" "(" expr ")" -> old_e
     | NAME -> var
     | "(" expr ")"
struct_fields: "{" NAME ":" expr ("," NAME ":" expr)* ","? "}"
expr_list: expr ("," expr)*

CMPOP: "==" | "!=" | "<=" | ">=" | "<" | ">"
NAME: /[a-zA-Z_][a-zA-Z_0-9]*/
INT: /[0-9]+/
COMMENT: /\/\/[^\n]*/
%import common.WS
%ignore WS
%ignore COMMENT
"""


def _loc(meta):
    if meta is None:
        return None
    return {"line": meta.line, "column": meta.column}


@v_args(inline=True, meta=True)
class Ast(Transformer):
    def NAME(self, *args):
        # With meta=True, rule nodes get (meta, ...); terminals pass only the token.
        return str(args[-1])

    def num(self, meta, n):
        return ("num", int(n))

    def true_lit(self, meta):
        return ("bool", True)

    def false_lit(self, meta):
        return ("bool", False)

    def none_lit(self, meta):
        return ("none",)

    def some_lit(self, meta, e):
        return ("some", e)

    def set_lit(self, meta, items=None):
        return ("set_lit", list(items or []))

    def struct_lit(self, meta, name, fields):
        return ("struct_lit", name, dict(fields))

    def struct_fields(self, meta, *pairs):
        out = []
        for i in range(0, len(pairs), 2):
            out.append((pairs[i], pairs[i + 1]))
        return out

    def var(self, meta, n):
        return ("var", n)

    def idx_suffix(self, meta, e):
        return ("idx", e)

    def field_suffix(self, meta, name):
        return ("field", name)

    def expr_list(self, meta, *exprs):
        return list(exprs)

    def _method_suffix(self, name, args=None):
        return ("method", name, list(args or []))

    def method_contains(self, meta, args=None):
        return self._method_suffix("contains", args)

    def method_add(self, meta, args=None):
        return self._method_suffix("add", args)

    def method_remove(self, meta, args=None):
        return self._method_suffix("remove", args)

    def method_size(self, meta):
        return self._method_suffix("size", [])

    def postfix(self, meta, atom, *suffixes):
        e = atom
        for s in suffixes:
            if s[0] == "idx":
                e = ("index", e, s[1])
            elif s[0] == "field":
                e = ("field", e, s[1])
            elif s[0] == "method":
                e = ("method", e, s[1], s[2])
        return e

    def add(self, meta, a, b):
        return ("bin", "+", a, b)

    def sub(self, meta, a, b):
        return ("bin", "-", a, b)

    def mul(self, meta, a, b):
        return ("bin", "*", a, b)

    def neg(self, meta, a):
        return ("neg", a)

    def cmp_op(self, meta, a, op, b):
        return ("bin", str(op), a, b)

    def and_op(self, meta, a, b):
        return ("bin", "and", a, b)

    def or_op(self, meta, a, b):
        return ("bin", "or", a, b)

    def imp(self, meta, a, b):
        return ("bin", "=>", a, b)

    def not_op(self, meta, a):
        return ("not", a)

    def is_pat(self, meta, a, pat=None):
        if pat is None:
            return a
        return ("is", a, pat)

    def pat_none(self, meta):
        return ("pat_none",)

    def pat_some(self, meta, name):
        return ("pat_some", name)

    def count_e(self, meta, v, ty, cond):
        return ("count", v, ty, cond)

    def sum_e(self, meta, v, ty, body, cond=None):
        return ("sum", v, ty, body, cond)

    def min_e(self, meta, a, b):
        return ("min", a, b)

    def max_e(self, meta, a, b):
        return ("max", a, b)

    def abs_e(self, meta, a):
        return ("abs", a)

    def old_e(self, meta, e):
        return ("old", e)

    def binder_typed(self, meta, v, ty, where=None):
        return ("binder_typed", v, ty, where)

    def binder_range(self, meta, v, lo, hi):
        return ("binder_range", v, lo, hi)

    def quant_forall(self, meta, b, e):
        return ("forall", b, e)

    def quant_forall_brace(self, meta, b, e):
        return ("forall", b, e)

    def quant_exists(self, meta, b, e):
        return ("exists", b, e)

    def quant_exists_brace(self, meta, b, e):
        return ("exists", b, e)

    def lvalue_var(self, meta, n):
        return ("var", n)

    def lvalue_index(self, meta, n, idx):
        return ("index", n, idx)

    def lvalue_field(self, meta, n, field):
        return ("field_lv", ("var", n), field)

    def lvalue_map_field(self, meta, n, idx, field):
        return ("field_lv", ("index", n, idx), field)

    def assign(self, meta, lv, e):
        return ("assign", lv, e, _loc(meta))

    def stmt_list(self, meta, *stmts):
        return [s for s in stmts if s is not None]

    def else_opt(self, meta, stmts):
        return stmts

    def if_stmt(self, meta, cond, then_stmts, else_stmts=None):
        return ("if", cond, then_stmts, else_stmts or [], _loc(meta))

    def forall_stmt(self, meta, b, stmts):
        return ("forall_stmt", b, stmts, _loc(meta))

    def requires_clause(self, meta, e):
        return ("requires", e, _loc(meta))

    def ensures_clause(self, meta, e):
        return ("ensures", e, _loc(meta))

    def let_clause(self, meta, n, e):
        return ("let", n, e, _loc(meta))

    def param_typed(self, meta, n, ty):
        return ("param_typed", n, ty)

    def param_range(self, meta, n, lo, hi):
        return ("param_range", n, lo, hi)

    def t_int(self, meta):
        return ("int",)

    def t_bool(self, meta):
        return ("bool",)

    def t_map(self, meta, k, v):
        return ("map", k, v)

    def t_set(self, meta, elem):
        return ("set", elem)

    def t_option(self, meta, inner):
        return ("option", inner)

    def t_name(self, meta, n):
        return ("name", n)

    def var_decl(self, meta, n, ty):
        return ("decl", n, ty)

    def enum_member(self, meta, name):
        return name

    def field(self, meta, n, ty):
        return (n, ty)

    def state_def(self, meta, *decls):
        return ("state", [d for d in decls if d])

    def const_def(self, meta, n, e):
        return ("const", n, e)

    def type_def(self, meta, n, lo, hi):
        return ("type", n, lo, hi)

    def enum_def(self, meta, n, *members):
        return ("enum", n, [m for m in members if m])

    def struct_def(self, meta, n, *fields):
        return ("struct", n, dict(fields))

    def init_def(self, meta, *stmts):
        return ("init", list(stmts))

    def invariant_def(self, meta, n, e):
        return ("invariant", n, e, _loc(meta))

    def reachable_def(self, meta, n, e):
        return ("reachable", n, e, _loc(meta))

    def action_def(self, meta, name, *rest):
        params, items = [], []
        for r in rest:
            if r is None:
                continue
            if r[0] in ("param_typed", "param_range"):
                params.append(r)
            else:
                items.append(r)
        return ("action", name, params, items, _loc(meta))

    def start(self, meta, name, *items):
        return ("spec", name, [i for i in items if i])


PARSER = Lark(
    GRAMMAR,
    parser="earley",
    maybe_placeholders=True,
    propagate_positions=True,
)
