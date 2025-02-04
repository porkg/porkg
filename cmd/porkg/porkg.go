package main

import (
	"context"
	"os"
	"time"

	"github.com/porkg/porkg/internal/worker"
	"github.com/rs/zerolog"
	"github.com/rs/zerolog/log"
)

func main() {
	zerolog.TimeFieldFormat = zerolog.TimeFormatUnix
	log.Logger = log.Output(zerolog.ConsoleWriter{Out: os.Stderr})
	worker.Reenter()

	ctx := context.Background()

	config := worker.WorkerConfig{}

	worker, err := worker.New(config)
	if err != nil {
		log.Fatal().
			Err(err).
			Msg("worker failure")
	}

	log.Info().
		Msg("worker started")

	defer func() {
		to, term := context.WithTimeout(ctx, time.Duration(time.Second*5))
		defer term()

		_, err := worker.Close(&to)
		if err != nil {
			log.Info().Err(err).Msg("worker failed")
		}
	}()

	job, err := worker.Start(&ctx)
	if err != nil {
		log.Fatal().
			Err(err).
			Msg("failed to start job")
	}
	log.Info().
		Msg("started job")

	job.Close()

	to, term := context.WithTimeout(ctx, time.Duration(time.Second*5))
	defer term()
	worker.Wait(&to)
}
