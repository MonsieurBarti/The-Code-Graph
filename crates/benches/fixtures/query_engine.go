package query

import (
	"fmt"
	"sort"
	"strings"
)

// Operator is a filter comparison operator.
type Operator string

const (
	OpEq         Operator = "eq"
	OpNeq        Operator = "neq"
	OpContains   Operator = "contains"
	OpStartsWith Operator = "starts_with"
	OpEndsWith   Operator = "ends_with"
)

// Filter describes a single filter predicate.
type Filter struct {
	Field    string
	Op       Operator
	Value    string
}

// SortOrder describes an ordering clause.
type SortOrder struct {
	Field     string
	Ascending bool
}

// QueryPlan is a structured query descriptor.
type QueryPlan struct {
	Filters []Filter
	Sort    []SortOrder
	Limit   int
	Offset  int
}

// QueryEngine executes queries over a slice of string maps.
type QueryEngine struct {
	records []map[string]string
}

// New creates a QueryEngine over the given records.
func New(records []map[string]string) *QueryEngine {
	return &QueryEngine{records: records}
}

// Execute runs the plan and returns matching records.
func (e *QueryEngine) Execute(plan QueryPlan) []map[string]string {
	result := make([]map[string]string, 0)
	for _, rec := range e.records {
		if matchesAll(rec, plan.Filters) {
			result = append(result, rec)
		}
	}
	if len(plan.Sort) > 0 {
		sort.SliceStable(result, func(i, j int) bool {
			for _, s := range plan.Sort {
				vi, vj := result[i][s.Field], result[j][s.Field]
				if vi == vj { continue }
				if s.Ascending { return vi < vj }
				return vi > vj
			}
			return false
		})
	}
	if plan.Offset > 0 && plan.Offset < len(result) { result = result[plan.Offset:] }
	if plan.Limit > 0 && plan.Limit < len(result) { result = result[:plan.Limit] }
	return result
}

func matchesAll(rec map[string]string, filters []Filter) bool {
	for _, f := range filters {
		v, ok := rec[f.Field]
		if !ok { return false }
		switch f.Op {
		case OpEq:         if v != f.Value { return false }
		case OpNeq:        if v == f.Value { return false }
		case OpContains:   if !strings.Contains(v, f.Value) { return false }
		case OpStartsWith: if !strings.HasPrefix(v, f.Value) { return false }
		case OpEndsWith:   if !strings.HasSuffix(v, f.Value) { return false }
		default:           return false
		}
	}
	return true
}

// Count returns the number of records matching the filters.
func (e *QueryEngine) Count(filters []Filter) int {
	n := 0
	for _, rec := range e.records {
		if matchesAll(rec, filters) { n++ }
	}
	return n
}

// Distinct returns unique values for a field.
func (e *QueryEngine) Distinct(field string) []string {
	seen := make(map[string]bool)
	var result []string
	for _, rec := range e.records {
		if v, ok := rec[field]; ok && !seen[v] {
			seen[v] = true
			result = append(result, v)
		}
	}
	sort.Strings(result)
	return result
}

// Explain returns a human-readable description of the plan.
func (e *QueryEngine) Explain(plan QueryPlan) string {
	return fmt.Sprintf("filters=%d sort=%d limit=%d offset=%d records=%d", len(plan.Filters), len(plan.Sort), plan.Limit, plan.Offset, len(e.records))
}
