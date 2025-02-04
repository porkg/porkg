//go:build linux

package worker

import (
	"context"
	"errors"
	"fmt"
	"os"
	"os/exec"
	"reflect"
	"sync/atomic"
	"syscall"

	"github.com/oraoto/go-pidfd"
	"github.com/porkg/porkg/internal/worker/proto"
	"github.com/rs/zerolog/log"
)

var rootToWorker = proto.CreateProtoMap(map[uint8]reflect.Type{
	1:   reflect.TypeFor[beginMessage](),
	2:   reflect.TypeFor[startMessage](),
	255: reflect.TypeFor[quitMessage](),
})

var workerToRoot = proto.CreateProtoMap(map[uint8]reflect.Type{
	1: reflect.TypeFor[startResult](),
})

var rootToJob = proto.CreateProtoMap(map[uint8]reflect.Type{
	1:   reflect.TypeFor[beginMessage](),
	255: reflect.TypeFor[quitMessage](),
})

var jobToRoot = proto.CreateProtoMap(map[uint8]reflect.Type{})

type WorkerConfig struct {
	Uid struct {
		Start  int `env:"UID_START"`
		Length int `env:"UID_LENGTH"`
	}
	Gid struct {
		Start  int `env:"GID_START"`
		Length int `env:"GID_LENGTH"`
	}
}

// This flag is hidden, nobody should be using it.
var isWorker = len(os.Args) == 2 && os.Args[1] == "--worker"
var isJob = len(os.Args) == 2 && os.Args[1] == "--job"
var isReentry = isWorker || isJob

func socketPair() (*os.File, *os.File, *os.File, *os.File, error) {
	recv, child_send, err := os.Pipe()
	if err != nil {
		return nil, nil, nil, nil, err
	}

	child_recv, send, err := os.Pipe()
	if err != nil {
		child_send.Close()
		recv.Close()
		return nil, nil, nil, nil, err
	}

	return recv, send, child_recv, child_send, nil
}

func New(config WorkerConfig) (*Worker, error) {
	current_exec, err := os.Executable()
	if err != nil {
		return nil, fmt.Errorf("failed to find the process to start for the worker: %w", err)
	}

	recv, send, child_recv, child_send, err := socketPair()
	if err != nil {
		return nil, fmt.Errorf("failed to create the pipe for the worker: %w", err)
	}
	defer child_send.Close()
	defer child_recv.Close()

	log.Info().
		Str("cmd", current_exec).
		Msg("starting worker process")

	cmd := exec.Command(current_exec, "--worker")
	cmd.Stderr = os.Stderr
	cmd.Stdin = os.Stdin
	cmd.Stdout = os.Stdout
	cmd.ExtraFiles = []*os.File{child_recv, child_send}
	cmd.SysProcAttr = &syscall.SysProcAttr{
		UidMappings: []syscall.SysProcIDMap{{
			ContainerID: 0,
			HostID:      syscall.Getuid(),
			Size:        1,
		}},
		GidMappings: []syscall.SysProcIDMap{{
			ContainerID: 0,
			HostID:      syscall.Getgid(),
			Size:        1,
		}},
		Cloneflags: syscall.CLONE_NEWPID | syscall.CLONE_NEWNS | syscall.CLONE_NEWUSER,
	}
	err = cmd.Start()

	if err != nil {
		send.Close()
		recv.Close()
		return nil, fmt.Errorf("failed to fork the worker: %w", err)
	}

	procFd, err := pidfd.Open(cmd.Process.Pid, 0)
	if err != nil {
		send.Close()
		recv.Close()
		return nil, fmt.Errorf("failed to get the worker process fd: %w", err)
	}

	z := &Worker{
		proc:      cmd.Process,
		procFd:    procFd,
		exitState: atomic.Pointer[os.ProcessState]{},
		died:      make(chan struct{}),
		send:      *send,
		recv:      *recv,
		proto:     proto.CreateProto(send, rootToWorker, recv, workerToRoot),
	}
	go z.monitorExit()

	log.Info().
		Int("pid", z.proc.Pid).
		Msg("started worker")

	parent := syscall.Getpid()
	err = z.proto.Send(beginMessage{
		Parent: parent,
	})

	if err != nil {
		z.Close(nil)
		return nil, err
	}

	return z, nil
}

type Worker struct {
	proc      *os.Process
	procFd    pidfd.PidFd
	exitState atomic.Pointer[os.ProcessState]
	exitError atomic.Pointer[error]
	died      chan struct{}
	send      os.File
	recv      os.File
	proto     proto.Proto
}

type Job struct {
	send  os.File
	recv  os.File
	proto proto.Proto
	proc  *os.Process
}

func (z *Worker) monitorExit() {
	defer close(z.died)

	exitState, err := z.proc.Wait()
	z.exitError.Store(&err)
	z.exitState.Store(exitState)

	if exitState == nil {
		return
	}

	if exitState.Success() {
		log.Info().Msg("worker process exited normally")
	} else if exitState.Exited() {
		log.Error().
			Int("exitCode", exitState.ExitCode()).
			Msg("worker process exited")
	} else {
		wait := exitState.Sys().(syscall.WaitStatus)
		if wait.Signaled() {
			if wait.Signal() == syscall.SIGTERM || wait.Signal() == syscall.SIGHUP {
				log.Info().
					Int("signal", int(wait.Signal())).
					Msg("worker process exited")
			} else {
				log.Error().
					Int("signal", int(wait.Signal())).
					Msg("worker process exited")
			}
		} else {
			log.Error().
				Str("status", fmt.Sprintf("%v", exitState)).
				Msg("worker process exited")
		}
	}
}

func (z *Worker) wait(ctx *context.Context) error {
	if ctx != nil {
		select {
		case <-z.died:
			return nil
		case <-(*ctx).Done():
			return (*ctx).Err()
		}
	}
	return nil
}

func (z *Worker) loadState() (*os.ProcessState, error) {
	proc := z.exitState.Load()
	err := z.exitError.Load()

	if err != nil {
		return nil, *err
	}
	return proc, nil
}

func (z *Worker) Start(ctx *context.Context) (*Job, error) {
	err := z.proto.Send(startMessage{})

	r, err := z.proto.Recv()
	if err != nil {
		return nil, err
	}

	res := r.(*startResult)

	sendFd, err := z.procFd.GetFd(res.WriteFd, 0)
	if err != nil {
		return nil, err
	}
	send := os.NewFile(uintptr(sendFd), "send")

	recvFd, err := z.procFd.GetFd(res.ReadFd, 0)
	if err != nil {
		send.Close()
		return nil, err
	}
	recv := os.NewFile(uintptr(recvFd), "recv")

	job := Job{
		send:  *send,
		recv:  *recv,
		proto: proto.CreateProto(send, rootToJob, recv, jobToRoot),
	}

	return &job, nil
}

func (z *Worker) Wait(ctx *context.Context) (*os.ProcessState, error) {
	if err := z.wait(ctx); err != nil {
		return nil, err
	}

	return z.loadState()
}

func (z *Worker) Close(ctx *context.Context) (*os.ProcessState, error) {
	z.proto.Send(quitMessage{})

	z.send.Close()
	z.recv.Close()

	z.proc.Signal(syscall.SIGTERM)
	if err := z.wait(ctx); err != nil {
		if errors.Is(err, context.DeadlineExceeded) || errors.Is(err, context.Canceled) {
			return nil, z.proc.Kill()
		}
		return nil, err
	}

	return z.loadState()
}

func Reenter() {
	if !isReentry {
		return
	}

	recv := os.NewFile(3, "pipe")
	if recv == nil {
		log.Fatal().Msg("failed to inherit receive pipe")
	}
	defer recv.Close()
	if _, err := recv.Read([]byte{}); err != nil {
		log.Fatal().Err(err).Msg("failed to inherit receive pipe")
	}

	send := os.NewFile(4, "pipe")
	if send == nil {
		log.Fatal().Msg("failed to inherit send pipe")
	}
	defer send.Close()
	if _, err := send.Write([]byte{}); err != nil {
		log.Fatal().Err(err).Msg("failed to inherit send pipe")
	}

	if isWorker {
		if err := reenterWorker(send, recv); err != nil {
			log.Fatal().Err(err).Msg("worker failed")
		}
	} else if isJob {
		if err := reenterJob(send, recv); err != nil {
			log.Fatal().Err(err).Msg("job failed")
		}
	} else {
		log.Fatal().Msg("unknown reentry type")
	}

	os.Exit(0)
}

func reenterWorker(send, recv *os.File) error {
	c := proto.CreateProto(send, workerToRoot, recv, rootToWorker)
	send, recv = nil, nil

	log.Info().Msg("starting worker message loop")
	for {
		msg, err := c.Recv()
		if err != nil {
			return err
		}

		if recv != nil {
			recv.Close()
			recv = nil
		}
		if send != nil {
			send.Close()
			send = nil
		}

		switch msg.(type) {
		case *beginMessage:
			log.Trace().
				Msg("switching to root")
			syscall.Setresuid(0, 0, 0)
			syscall.Setresgid(0, 0, 0)
		case *startMessage:
			log.Trace().
				Msg("received start message")

			var child_recv, child_send *os.File

			recv, send, child_recv, child_send, err = socketPair()
			if err != nil {
				return fmt.Errorf("failed to create the job pipe: %w", err)
			}

			defer child_recv.Close()
			defer child_send.Close()

			current_exec, err := os.Executable()
			if err != nil {
				return fmt.Errorf("failed to find the process to start for the job: %w", err)
			}

			cmd := exec.Command(current_exec, "--job")
			cmd.Stderr = os.Stderr
			cmd.Stdin = os.Stdin
			cmd.Stdout = os.Stdout
			cmd.ExtraFiles = []*os.File{child_recv, child_send}
			cmd.SysProcAttr = &syscall.SysProcAttr{
				Cloneflags: syscall.CLONE_NEWUSER | syscall.CLONE_NEWNS | syscall.CLONE_NEWUSER,
			}
			err = cmd.Start()

			if err != nil {
				send.Close()
				recv.Close()
				return fmt.Errorf("failed to fork the job: %w", err)
			}

			err = c.Send(startResult{
				ReadFd:  int(recv.Fd()),
				WriteFd: int(send.Fd()),
				Pid:     cmd.Process.Pid,
			})

			if err != nil {
				return err
			}
		case *quitMessage:
			log.Info().Msg("Exiting")
			return nil
		default:
			return fmt.Errorf("invalid worker-bound message: %q", reflect.TypeOf(msg))
		}
	}
}

func reenterJob(send, recv *os.File) error {
	c := proto.CreateProto(send, workerToRoot, recv, rootToWorker)

	log.Info().Msg("starting job message loop")
	for {
		msg, err := c.Recv()
		if err != nil {
			return err
		}

		switch msg.(type) {
		case *beginMessage:
			log.Trace().
				Msg("job switching to root")
			syscall.Setresuid(0, 0, 0)
			syscall.Setresgid(0, 0, 0)
		case *quitMessage:
			log.Info().Msg("Job exiting")
			return nil
		default:
			return fmt.Errorf("invalid job-bound message: %q", reflect.TypeOf(msg))
		}
	}
}

type beginMessage struct {
	Parent int
}

type quitMessage struct{}

type startMessage struct{}

type startResult struct {
	ReadFd  int
	WriteFd int
	Pid     int
}

func (j *Job) Close() {
	j.proto.Send(quitMessage{})
}
