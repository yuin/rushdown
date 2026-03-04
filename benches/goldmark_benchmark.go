package main

import (
	"bytes"
	"fmt"
	"os"
	"strconv"
	"time"

	"github.com/yuin/goldmark"
	"github.com/yuin/goldmark/renderer/html"
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
	markdown := goldmark.New(goldmark.WithRendererOptions(html.WithXHTML(), html.WithUnsafe()))
	var out bytes.Buffer
	markdown.Convert([]byte(""), &out)

	sum := time.Duration(0)
	for i := 0; i < n; i++ {
		start := time.Now()
		out.Reset()
		if err := markdown.Convert(source, &out); err != nil {
			panic(err)
		}
		sum += time.Since(start)
	}
	fmt.Print("\033[32mgoldmark\033[0m\t")
	fmt.Printf("time: %.4f ms\n", float64((int64(sum)/int64(n)))/1000000.0)
}
