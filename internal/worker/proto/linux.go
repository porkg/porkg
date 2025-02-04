//go:build linux

package proto

import (
	"encoding/binary"
	"fmt"
	"io"
	"math"
	"reflect"

	"github.com/rs/zerolog/log"
	"github.com/vmihailenco/msgpack/v5"
)

type readResult struct {
	msg interface{}
	err error
}

type Proto struct {
	order      binary.ByteOrder
	writer     io.Writer
	writerTags *ProtoTagMap

	reader chan readResult
}

func CreateProto(writer io.Writer, writerTags *ProtoTagMap, reader io.Reader, readerTags *ProtoTagMap) Proto {
	r := make(chan readResult)
	c := Proto{
		order:      binary.BigEndian,
		writer:     writer,
		writerTags: writerTags,
		reader:     r,
	}

	go c.recvWorker(reader, readerTags)
	return c
}

func (c *Proto) Send(data interface{}) error {
	buffer, err := msgpack.Marshal(data)
	if err != nil {
		return fmt.Errorf("failed to marshal message: %w", err)
	}

	bufLen := len(buffer)
	if bufLen > math.MaxUint32 {
		return fmt.Errorf("failed to marshal message: message too large")
	}

	var ok bool
	headerBuf := make([]byte, 5)
	if headerBuf[0], ok = c.writerTags.toTag[reflect.TypeOf(data)]; !ok {
		return fmt.Errorf("unknown message type: %s", reflect.TypeOf(data).Name())
	}

	c.order.PutUint32(headerBuf[1:], uint32(bufLen))
	err = c.sendBytes(headerBuf[:])
	if err != nil {
		return err
	}

	err = c.sendBytes(buffer)
	if err != nil {
		return err
	}

	return nil
}

func (c *Proto) Recv() (interface{}, error) {
	result := <-c.reader
	if result.err != nil {
		return nil, fmt.Errorf("failed to read message: %w", result.err)
	}

	return result.msg, nil
}

func (c *Proto) sendBytes(data []byte) error {
	log.Trace().Bytes("data", data[:]).Msg("sending bytes")
	for len(data) != 0 {
		n, err := c.writer.Write(data)
		if err != nil {
			return fmt.Errorf("failed to send message: %w", err)
		}
		if n == 0 {
			return fmt.Errorf("failed to send message: stream closed")
		}

		log.Trace().Int("length", n).Msg("sent bytes")
		data = data[n:]
	}

	return nil
}

func (c *Proto) recvWorker(reader io.Reader, tags *ProtoTagMap) {
	defer close(c.reader)
	headerBuf := make([]byte, 5)
	dataBuf := make([]byte, 0)

	for {
		err := recvBytes(reader, headerBuf)
		if err != nil {
			c.reader <- readResult{err: err}
			return
		}

		l := c.order.Uint32(headerBuf[1:])

		log.Trace().
			Uint32("length", l).
			Uint8("type", headerBuf[0]).
			Msg("reading raw message")

		if len(dataBuf) < int(l) {
			dataBuf = make([]byte, l)
		}

		err = recvBytes(reader, dataBuf[:l])
		if err != nil {
			c.reader <- readResult{err: err}
			return
		}

		t, ok := tags.toType[headerBuf[0]]
		if !ok {
			err = fmt.Errorf("unknown tag %q", headerBuf[0])
			c.reader <- readResult{err: err}
			return
		}

		val := reflect.New(t).Interface()
		err = msgpack.Unmarshal(dataBuf[:l], val)
		if err != nil {
			err = fmt.Errorf("failed to unmarshal %q message: %w", reflect.TypeOf(val).Name(), err)
			c.reader <- readResult{err: err}
			return
		}

		if len(dataBuf) > (1024 * 1024) {
			dataBuf = make([]byte, 0)
		}

		c.reader <- readResult{msg: val}
	}
}

func recvBytes(reader io.Reader, data []byte) error {
	log.Trace().Int("length", len(data)).Msg("receiving bytes")
	for len(data) != 0 {
		n, err := reader.Read(data)

		if err != nil {
			return fmt.Errorf("failed to receive bytes: %w", err)
		}

		log.Trace().Bytes("data", data[:n]).Msg("received bytes")
		data = data[n:]
	}

	return nil
}
