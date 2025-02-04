package proto

import "reflect"

type ProtoTagMap struct {
	toTag  map[reflect.Type]uint8
	toType map[uint8]reflect.Type
}

func CreateProtoMap(toType map[uint8]reflect.Type) *ProtoTagMap {
	toTag := make(map[reflect.Type]uint8)

	for tag, ty := range toType {
		toTag[ty] = tag
	}

	return &ProtoTagMap{
		toTag:  toTag,
		toType: toType,
	}
}
