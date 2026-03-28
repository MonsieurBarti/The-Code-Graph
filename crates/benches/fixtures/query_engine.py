from __future__ import annotations

import re
from dataclasses import dataclass
from enum import Enum, auto
from typing import Any, Callable, Optional


class Operator(Enum):
    EQ = auto()
    NEQ = auto()
    LT = auto()
    LTE = auto()
    GT = auto()
    GTE = auto()
    CONTAINS = auto()
    STARTS_WITH = auto()
    ENDS_WITH = auto()


@dataclass
class Filter:
    field: str
    operator: Operator
    value: Any

    def matches(self, record: dict[str, Any]) -> bool:
        field_val = record.get(self.field)
        if field_val is None:
            return False
        match self.operator:
            case Operator.EQ: return field_val == self.value
            case Operator.NEQ: return field_val != self.value
            case Operator.LT: return field_val < self.value
            case Operator.LTE: return field_val <= self.value
            case Operator.GT: return field_val > self.value
            case Operator.GTE: return field_val >= self.value
            case Operator.CONTAINS: return self.value in str(field_val)
            case Operator.STARTS_WITH: return str(field_val).startswith(self.value)
            case Operator.ENDS_WITH: return str(field_val).endswith(self.value)
        return False


@dataclass
class QueryPlan:
    filters: list[Filter]
    order_by: Optional[str] = None
    descending: bool = False
    limit: Optional[int] = None
    offset: int = 0


class QueryEngine:
    def __init__(self, records: list[dict[str, Any]]) -> None:
        self._records = records

    def execute(self, plan: QueryPlan) -> list[dict[str, Any]]:
        result = [r for r in self._records if all(f.matches(r) for f in plan.filters)]
        if plan.order_by:
            result.sort(key=lambda r: r.get(plan.order_by, None), reverse=plan.descending)
        result = result[plan.offset:]
        if plan.limit is not None:
            result = result[:plan.limit]
        return result

    def count(self, filters: list[Filter]) -> int:
        return sum(1 for r in self._records if all(f.matches(r) for f in filters))

    def distinct(self, field: str) -> list[Any]:
        seen: set[Any] = set()
        result = []
        for r in self._records:
            v = r.get(field)
            if v not in seen:
                seen.add(v)
                result.append(v)
        return result
