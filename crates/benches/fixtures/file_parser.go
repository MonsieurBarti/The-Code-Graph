package parser

import (
	"go/ast"
	"go/parser"
	"go/token"
	"path/filepath"
)

// FuncDecl holds information about a parsed function.
type FuncDecl struct {
	Name     string
	Receiver string
	Line     int
	IsExported bool
}

// TypeDecl holds information about a parsed type.
type TypeDecl struct {
	Name   string
	Kind   string // struct, interface, alias
	Line   int
}

// ImportDecl holds information about a parsed import.
type ImportDecl struct {
	Path  string
	Alias string
	Line  int
}

// ParseResult aggregates all parsed declarations.
type ParseResult struct {
	Functions []FuncDecl
	Types     []TypeDecl
	Imports   []ImportDecl
	Errors    []string
}

// ParseFile parses a single Go source file.
func ParseFile(path string) (*ParseResult, error) {
	fset := token.NewFileSet()
	f, err := parser.ParseFile(fset, path, nil, parser.AllErrors)
	if err != nil {
		return &ParseResult{Errors: []string{err.Error()}}, nil
	}
	result := &ParseResult{}
	for _, imp := range f.Imports {
		p := imp.Path.Value
		alias := ""
		if imp.Name != nil {
			alias = imp.Name.Name
		}
		result.Imports = append(result.Imports, ImportDecl{
			Path:  p,
			Alias: alias,
			Line:  fset.Position(imp.Pos()).Line,
		})
	}
	ast.Inspect(f, func(n ast.Node) bool {
		switch decl := n.(type) {
		case *ast.FuncDecl:
			recv := ""
			if decl.Recv != nil && len(decl.Recv.List) > 0 {
				recv = filepath.Base(decl.Name.String())
			}
			result.Functions = append(result.Functions, FuncDecl{
				Name:       decl.Name.Name,
				Receiver:   recv,
				Line:       fset.Position(decl.Pos()).Line,
				IsExported: ast.IsExported(decl.Name.Name),
			})
		case *ast.TypeSpec:
			kind := "alias"
			switch decl.Type.(type) {
			case *ast.StructType:
				kind = "struct"
			case *ast.InterfaceType:
				kind = "interface"
			}
			result.Types = append(result.Types, TypeDecl{
				Name: decl.Name.Name,
				Kind: kind,
				Line: fset.Position(decl.Pos()).Line,
			})
		}
		return true
	})
	return result, nil
}
