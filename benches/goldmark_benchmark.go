package main

import (
	"bytes"
	"fmt"
	"os"
	"strconv"
	"time"

	"github.com/yuin/goldmark/v2/parser"
	"github.com/yuin/goldmark/v2/renderer/html"
)

func main() {
	n := 500
	file := "fixtures/data.md"
	if len(os.Args) > 1 {
		n, _ = strconv.Atoi(os.Args[1])
	}
	if len(os.Args) > 2 {
		file = os.Args[2]
	}
	source, err := os.ReadFile(file)
	if err != nil {
		panic(err)
	}
	p := parser.New()
	r := html.New(html.WithXHTML(), html.WithUnsafe())
	var out bytes.Buffer

	sum := time.Duration(0)
	for i := 0; i < n; i++ {
		start := time.Now()
		out.Reset()
		doc := p.Parse(source)
		if err := r.Render(&out, source, doc); err != nil {
			panic(err)
		}
		sum += time.Since(start)
	}
	fmt.Print("\033[32mgoldmark\033[0m\t")
	fmt.Printf("time: %.4f ms\n", float64((int64(sum)/int64(n)))/1000000.0)
}
