package main

import (
	"context"
	"log"
	"time"

	"github.com/porkg/porkg/internal/zygote"
)

func main() {
	ctx := context.Background()

	zygote, err := zygote.Spawn()
	if err != nil {
		log.Fatalf("Zygote failure: %v", err)
	}

	if zygote == nil {
		log.Println("Zygote exiting")
		return
	}

	log.Println("Zygote spawned")

	defer func() {
		to, term := context.WithTimeout(ctx, time.Duration(time.Second*5))
		defer term()

		_, err := zygote.Close(&to)
		if err != nil {
			log.Printf("Zygote failed: %v", err)
		}
	}()

	to, term := context.WithTimeout(ctx, time.Duration(time.Second*5))
	defer term()
	zygote.Wait(&to)
}
