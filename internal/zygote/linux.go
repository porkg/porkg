//go:build linux

package zygote

import (
	"context"
	"errors"
	"fmt"
	"log/slog"
	"os"
	"strconv"
	"sync/atomic"
	"syscall"
)

func Spawn() (*Zygote, error) {
	if len(os.Args) >= 1 && os.Args[0] == "--zygote" {
		if len(os.Args) >= 3 {
			slog.Info("becoming zygote")
			return nil, becomeZygote()
		}

		return nil, fmt.Errorf("Invalid number of args")
	}

	current_exec, err := os.Executable()
	if err != nil {
		return nil, fmt.Errorf("failed to find the process to start for the zygote: %w", err)
	}

	attr := syscall.ProcAttr{}

	if attr.Dir, err = os.Getwd(); err != nil {
		attr.Dir = "/"
	}

	send, child_recv, err := os.Pipe()
	if err != nil {
		return nil, fmt.Errorf("failed to create the send pipe for the zygote: %w", err)
	}

	defer child_recv.Close()

	child_send, recv, err := os.Pipe()
	if err != nil {
		send.Close()
		return nil, fmt.Errorf("failed to create the receive pipe for the zygote: %w", err)
	}

	defer child_send.Close()

	send_fd := child_send.Fd()
	recv_fd := child_recv.Fd()

	attr.Env = os.Environ()
	attr.Files = []uintptr{os.Stdin.Fd(), os.Stdout.Fd(), os.Stderr.Fd(), send_fd, recv_fd}
	// attr.Sys = &syscall.SysProcAttr{
	// 	Cloneflags: syscall.CLONE_NEWUTS | syscall.CLONE_NEWUSER,
	// }

	send_fd_str := strconv.FormatUint(uint64(send_fd), 16)
	recv_fd_str := strconv.FormatUint(uint64(recv_fd), 16)

	slog.Debug("starting zygote process", "cmd", current_exec)
	pid, err := syscall.ForkExec(current_exec, []string{"--zygote", send_fd_str, recv_fd_str}, &attr)

	if err != nil {
		recv.Close()
		send.Close()
		return nil, fmt.Errorf("failed to fork the zygote: %w", err)
	}

	proc, err := os.FindProcess(pid)
	if err != nil {
		// Doesn't happen on unix, but code correctly regardless
		recv.Close()
		send.Close()
		return nil, fmt.Errorf("failed to get the zygote process: %w", err)
	}

	z := &Zygote{
		proc:      proc,
		exitState: atomic.Pointer[os.ProcessState]{},
		died:      make(chan struct{}),
		send:      send,
		recv:      recv,
	}
	go z.monitorExit()
	return z, nil
}

type Zygote struct {
	proc      *os.Process
	exitState atomic.Pointer[os.ProcessState]
	exitError atomic.Pointer[error]
	died      chan struct{}
	send      *os.File
	recv      *os.File
}

func (z *Zygote) monitorExit() {
	defer close(z.died)

	exitState, err := z.proc.Wait()
	z.exitError.Store(&err)
	z.exitState.Store(exitState)

	if exitState == nil {
		return
	}

	if exitState.Success() {
		slog.Info("zygote process exited normally")
	} else if exitState.Exited() {
		slog.Error("zygote process exited", "exitCode", exitState.ExitCode())
	} else {
		wait := exitState.Sys().(syscall.WaitStatus)
		if wait.Signaled() {
			if wait.Signal() == syscall.SIGTERM || wait.Signal() == syscall.SIGHUP {
				slog.Info("zygote process exited", "signal", wait.Signal())
			} else {
				slog.Error("zygote process exited", "signal", wait.Signal())
			}
		} else {
			slog.Error("zygote process exited", "status", exitState)
		}
	}
}

func (z *Zygote) wait(ctx *context.Context) error {
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

func (z *Zygote) loadState() (*os.ProcessState, error) {
	proc := z.exitState.Load()
	err := z.exitError.Load()

	if err != nil {
		return nil, *err
	}
	return proc, nil
}

func (z *Zygote) Wait(ctx *context.Context) (*os.ProcessState, error) {
	if err := z.wait(ctx); err != nil {
		return nil, err
	}

	return z.loadState()
}

func (z *Zygote) Close(ctx *context.Context) (*os.ProcessState, error) {
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

func becomeZygote() error {
	send, err := inheritFd(os.Args[1])
	if err != nil {
		return fmt.Errorf("failed to get send pipe: %w", err)
	}
	defer send.Close()

	recv, err := inheritFd(os.Args[2])
	if err != nil {
		return fmt.Errorf("failed to get receive pipe: %w", err)
	}
	defer recv.Close()

	return nil
}

func inheritFd(fdStr string) (*os.File, error) {
	fd, err := strconv.ParseUint(fdStr, 16, 64)
	if err != nil {
		return nil, fmt.Errorf("invalid FD '%v': %w", fdStr, err)
	}

	pipe := os.NewFile(uintptr(fd), "pipe")
	if pipe == nil {
		return nil, fmt.Errorf("failed to inherit FD '%v': failed to resolve pipe", fd)
	}

	return pipe, nil
}
