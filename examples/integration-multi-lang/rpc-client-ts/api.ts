/* eslint-disable */
import Long from "long";
import _m0 from "protobufjs/minimal";

export const protobufPackage = "";

export interface Book {
  isbn: number;
  title: string;
  author: string;
}

export interface GetBookRequest {
  isbn: number;
}

export interface QueryBooksRequest {
  authorPrefix: string;
}

function createBaseBook(): Book {
  return { isbn: 0, title: "", author: "" };
}

export const Book = {
  encode(message: Book, writer: _m0.Writer = _m0.Writer.create()): _m0.Writer {
    if (message.isbn !== 0) {
      writer.uint32(8).int64(message.isbn);
    }
    if (message.title !== "") {
      writer.uint32(18).string(message.title);
    }
    if (message.author !== "") {
      writer.uint32(26).string(message.author);
    }
    return writer;
  },

  decode(input: _m0.Reader | Uint8Array, length?: number): Book {
    const reader = input instanceof _m0.Reader ? input : new _m0.Reader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message = createBaseBook();
    while (reader.pos < end) {
      const tag = reader.uint32();
      switch (tag >>> 3) {
        case 1:
          message.isbn = longToNumber(reader.int64() as Long);
          break;
        case 2:
          message.title = reader.string();
          break;
        case 3:
          message.author = reader.string();
          break;
        default:
          reader.skipType(tag & 7);
          break;
      }
    }
    return message;
  },

  fromJSON(object: any): Book {
    return {
      isbn: isSet(object.isbn) ? Number(object.isbn) : 0,
      title: isSet(object.title) ? String(object.title) : "",
      author: isSet(object.author) ? String(object.author) : "",
    };
  },

  toJSON(message: Book): unknown {
    const obj: any = {};
    message.isbn !== undefined && (obj.isbn = Math.round(message.isbn));
    message.title !== undefined && (obj.title = message.title);
    message.author !== undefined && (obj.author = message.author);
    return obj;
  },

  fromPartial<I extends Exact<DeepPartial<Book>, I>>(object: I): Book {
    const message = createBaseBook();
    message.isbn = object.isbn ?? 0;
    message.title = object.title ?? "";
    message.author = object.author ?? "";
    return message;
  },
};

function createBaseGetBookRequest(): GetBookRequest {
  return { isbn: 0 };
}

export const GetBookRequest = {
  encode(message: GetBookRequest, writer: _m0.Writer = _m0.Writer.create()): _m0.Writer {
    if (message.isbn !== 0) {
      writer.uint32(8).int64(message.isbn);
    }
    return writer;
  },

  decode(input: _m0.Reader | Uint8Array, length?: number): GetBookRequest {
    const reader = input instanceof _m0.Reader ? input : new _m0.Reader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message = createBaseGetBookRequest();
    while (reader.pos < end) {
      const tag = reader.uint32();
      switch (tag >>> 3) {
        case 1:
          message.isbn = longToNumber(reader.int64() as Long);
          break;
        default:
          reader.skipType(tag & 7);
          break;
      }
    }
    return message;
  },

  fromJSON(object: any): GetBookRequest {
    return { isbn: isSet(object.isbn) ? Number(object.isbn) : 0 };
  },

  toJSON(message: GetBookRequest): unknown {
    const obj: any = {};
    message.isbn !== undefined && (obj.isbn = Math.round(message.isbn));
    return obj;
  },

  fromPartial<I extends Exact<DeepPartial<GetBookRequest>, I>>(object: I): GetBookRequest {
    const message = createBaseGetBookRequest();
    message.isbn = object.isbn ?? 0;
    return message;
  },
};

function createBaseQueryBooksRequest(): QueryBooksRequest {
  return { authorPrefix: "" };
}

export const QueryBooksRequest = {
  encode(message: QueryBooksRequest, writer: _m0.Writer = _m0.Writer.create()): _m0.Writer {
    if (message.authorPrefix !== "") {
      writer.uint32(10).string(message.authorPrefix);
    }
    return writer;
  },

  decode(input: _m0.Reader | Uint8Array, length?: number): QueryBooksRequest {
    const reader = input instanceof _m0.Reader ? input : new _m0.Reader(input);
    let end = length === undefined ? reader.len : reader.pos + length;
    const message = createBaseQueryBooksRequest();
    while (reader.pos < end) {
      const tag = reader.uint32();
      switch (tag >>> 3) {
        case 1:
          message.authorPrefix = reader.string();
          break;
        default:
          reader.skipType(tag & 7);
          break;
      }
    }
    return message;
  },

  fromJSON(object: any): QueryBooksRequest {
    return { authorPrefix: isSet(object.authorPrefix) ? String(object.authorPrefix) : "" };
  },

  toJSON(message: QueryBooksRequest): unknown {
    const obj: any = {};
    message.authorPrefix !== undefined && (obj.authorPrefix = message.authorPrefix);
    return obj;
  },

  fromPartial<I extends Exact<DeepPartial<QueryBooksRequest>, I>>(object: I): QueryBooksRequest {
    const message = createBaseQueryBooksRequest();
    message.authorPrefix = object.authorPrefix ?? "";
    return message;
  },
};

export type BookServiceDefinition = typeof BookServiceDefinition;
export const BookServiceDefinition = {
  name: "BookService",
  fullName: "BookService",
  methods: {
    getBook: {
      name: "GetBook",
      requestType: GetBookRequest,
      requestStream: false,
      responseType: Book,
      responseStream: false,
      options: {},
    },
    queryBooks: {
      name: "QueryBooks",
      requestType: QueryBooksRequest,
      requestStream: false,
      responseType: Book,
      responseStream: true,
      options: {},
    },
    getBookStream: {
      name: "GetBookStream",
      requestType: GetBookRequest,
      requestStream: true,
      responseType: Book,
      responseStream: false,
      options: {},
    },
    queryBooksStream: {
      name: "QueryBooksStream",
      requestType: GetBookRequest,
      requestStream: true,
      responseType: Book,
      responseStream: true,
      options: {},
    },
  },
} as const;

declare var self: any | undefined;
declare var window: any | undefined;
declare var global: any | undefined;
var globalThis: any = (() => {
  if (typeof globalThis !== "undefined") {
    return globalThis;
  }
  if (typeof self !== "undefined") {
    return self;
  }
  if (typeof window !== "undefined") {
    return window;
  }
  if (typeof global !== "undefined") {
    return global;
  }
  throw "Unable to locate global object";
})();

type Builtin = Date | Function | Uint8Array | string | number | boolean | undefined;

export type DeepPartial<T> = T extends Builtin ? T
  : T extends Array<infer U> ? Array<DeepPartial<U>> : T extends ReadonlyArray<infer U> ? ReadonlyArray<DeepPartial<U>>
  : T extends {} ? { [K in keyof T]?: DeepPartial<T[K]> }
  : Partial<T>;

type KeysOfUnion<T> = T extends T ? keyof T : never;
export type Exact<P, I extends P> = P extends Builtin ? P
  : P & { [K in keyof P]: Exact<P[K], I[K]> } & { [K in Exclude<keyof I, KeysOfUnion<P>>]: never };

function longToNumber(long: Long): number {
  if (long.gt(Number.MAX_SAFE_INTEGER)) {
    throw new globalThis.Error("Value is larger than Number.MAX_SAFE_INTEGER");
  }
  return long.toNumber();
}

if (_m0.util.Long !== Long) {
  _m0.util.Long = Long as any;
  _m0.configure();
}

function isSet(value: any): boolean {
  return value !== null && value !== undefined;
}