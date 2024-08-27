# recapn-rpc

This is the RPC side of capnp. This crate provides the main implementation of RPC much like the capnp-rpc crate.

Nothing in this crate is implemented in a similar way to the C++ or existing Rust implementations. It has been explicitly designed with the goal of avoiding the issues I have with their implementation. The issues I personally have are:

1. No support for work-stealing multi-threading.
2. No usable isolated connection abstraction.
3. no multi-thread :(

The C++ implementation of RPC was designed for one specific environment: KJ async. KJ async is a (in)famously a single-threaded runtime! So it doesn't need to support multi-threading. Supporting multi-threading would likely be much harder because it'd have to work around the design of KJ async which is extremely single-threaded in it's design.

The Rust implementation of RPC is a copy of the C++ implementation as always. It doesn't copy KJ async luckily so it uses tokio as a base. This however introduces some confusion in the Rust community. Tokio supports multi-threading, so why doesn't capnp-rpc support multi-threading? Well, it copies an implementation that was designed for an environment that is exclusively single-threaded!

The design that C++ uses is simply wrong. For multi-threading that is. To have multi-threaded RPC, we'll need to take a step back and build a new implementation from scratch. So to do that, we need to first talk about what Cap'n Proto RPC even is.

## A refresher

Cap'n Proto RPC is an RPC framework based on E-lang, backed by Cap'n Proto serialization. RPC takes from E-lang and creates an easy to use system of distributed objects or "capabilities" (or "caps") in a network of "vats".

This is probably where Cap'n Proto gets its name. "Cap(abilities)" "(a)'n(d)" "proto(col buffers)" or something I dunno.

In Cap'n Proto RPC, you construct an RPC system. This RPC system contains a set of connections with capabilities. These connections are to nodes (called "vats") in the network. A "vat" could, for example, be a thread, a process, or another machine. A "vat" contains a set of capabilities specific to that vat. Capabilities are effectively just objects. Most of the time, you start with a "bootstrap" capability that lets you make requests to a set of methods. Those methods then return responses which can contain more capabilities for you to call more methods on.

RPC guarantees that messages sent to a specific capability "reference" will arrive in a specified order, called "E order" (also known as first-in first-out). Even if the RPC system is able to shorten the path between capabilities, it always maintains this order of requests. This is extremely important to correctness of the system.

Seems simple enough. It sounds like gRPC but with explicit objects that the library keeps track of for you. How neat! But there's a few features that make it more complicated.

### Sending capabilities

Capabilities can be sent between two parties. If Vat A sends a capability to Vat B, B can then make requests on that capability. But Vat B can send capabilities from A *back to them*. Cap'n Proto automatically handles this case on the side of A by handling requests to this capability as though it's all happening locally. No need for round trip!

### Pipelining (and the ordering problem)

RPC also includes a feature called "promise pipelining". Promise pipelining allows you to create requests for capabilities *before you've received them*, allowing you to avoid waiting for a round-trip request from the other capability, which could have high latency! This lets you optimize the happy path and reduce latency when everything works successfully.

But now we have a problem: If a capability has requests sent to it through promise pipelining, and that capability then turns out to be hosted locally by the original sender of the requests, then those requests could still be somewhere in the network when we start making new local requests. We could make local requests that are received locally *before* the requests we made before optimistically which would violate E-order!

Luckily, the protocol describes a method of keeping order between requests. The system imposes what's called an "embargo" on requests to the capability and then sends a disembargo request around the system on the original path of the sent messages. The receiving vat sends the disembargo back once it has finished processing and sending the original pipelined requests back. Thus, when the disembargo message comes back to the original vat, it knows it has received all the original requests sent to the other machine.

## Back to C++

Now that we remember what RPC is and the difficulties associated with implementing it, we can talk about what the C++ implementation does. Or rather, we can talk about what C++ *doesn't do*.

C++ defines RPC in terms of a set of abstract interfaces called "hooks". Client hooks define the internal interface for capabilities. Request hooks define the internal interface for requests. Response hooks for responses. Pipeline hooks for pipelines. Call context hooks for local calls. For every part of the system there's a customizable hook that must be implemented. RPC then implements these hooks for each of the features. Local calls are implemented as "local" types. RPC implements it's own types. There's also separate implementations for certain features such as membranes and retryable calls. This interface is designed so that you can implement RPC however you want, including not implementing it at all! But it must *always* be defined in terms of these abstract hooks, since that's what's used by the serialization library to integrate itself with RPC.

This interface is extremely basic, it only defines an API for the basic operations. For clients you can make new requests, send existing request instances, add references to the client, or try to wait for resolution. For requests you can send them or build the parameters. For calls you can drop parameters, build the response, or setup a pipeline. That's all. It's an abstract interface for the basic operations you're expected to support.

This design leaves a bit to be desired. For one: *everything requires a heap allocation*. Everything is an abstract interface using dynamic dispatch and you need to allocate on the heap to create instances of your hooks. Clients, requests, responses, pipelines, and call contexts are all heap allocations. That sucks! And don't even get me started on pipelines. Pipelines themselves create a copy of the entire pipeline ops set every time you append a new operation. Lots of time in C++ RPC is spent just allocating stuff. Some allocations are obviously going to be needed, but there's plenty of room for improvement. Though sadly there's not a lot of space to use due to the design of the API.

Outside of allocations, another place this design falters is it repeating itself. There's at least 3 different ways clients buffer requests for 3 different types. Local clients buffer requests on blocking streaming calls using a linked list. Promised clients buffer requests by using KJ async "then()" continuations (knowing that KJ async will invoke continuations in the order they were made). RPC promised clients do the same thing, except with support for RpcClient interfaces (an extended ClientHook interface for RPC stuff). And of course every client that can resolve into something else will have logic for itself resolving into something else. This logic might be fine if it was all grouped together, but it's all spread apart! There's 7 implementations of ClientHook across 4 files and they all behave *mostly* the same but the devil is in the details. You need to know those details because how they all behave is critical to maintaining message ordering!

The Rust implementation isn't worth mentioning because it inherits all the problems of the C++ implementation *and then some*. Remember how everything is abstracted hooks? Well those abstractions also are abstractions over *reference counting*. This abstraction only really works in KJ, where the `Own` type is an extremely abstract kind of ownership. It's not a `Box`, an `Rc`, or an `Arc`, it's just a pointer to some data and a pointer to a deallocator. *That's it.* Though I will admit it's kinda cool! You can have something that's either single-owner or ref counted with the same abstraction! You can even have a derived value that owned by a ref counted parent, but all the receiver sees is just a `Own<T>`.

*This does not work in Rust.* At least not without making a separate type. capnp-rust probably could copy this and make something like `kj::Own`, but they didn't. So now Rust has two problems:

1. Everything allocates.
2. *Everything allocates.* ***AGAIN.***

The client hook interface requires a trait object, so it has to be hidden by something like a `Box` or an `Rc`. But because it copies C++ directly, it's also copying the *ownership abstraction*. So you get stuff like `Box<dyn ClientHook>` where the client hook box is then backed by *another box* in the form of an `Rc`. Every. Single. Time. So every allocation you make also makes *another* allocation to box **another box**.

Let's not even think about how much this effects optimization. Can't optimize your way past a vtable (ignoring optimizations that do in very specific cases). No inlining opportunities. You just have to do dynamic dispatch for most everything in RPC land.

## Approaching from a different angle

Let's take a different approach. Throw everything C++ does out. Throw everything capnp-rust does out. We have a spec, we know what RPC needs to do. We just need to think of a different way of doing it.

To recap the issues we have to tackle:

 * Excessive allocations / abstraction
 * No multi-threading support within a connection, which makes it harder to integrate with the rest of the Rust community
 * No common system for request handling and maintaining order

The main problem we have to solve with RPC is message ordering. How do we maintain message ordering in a clean, fast, and easy way? Ideally how can we include support for multi-threading?

Given all we know of RPC, it's easy to draw parallels to other well known data structures that are commonly used outside of Cap'n Proto. Capabilities (and references) are essentially FIFO multi-producer single-consumer channels that can be forwarded to other channels. Simple enough! MPSC channels are an extremely common abstraction used in services today. There's at least 5(00) implementations in Rust. Go is practically built upon them. I know they were being introduced to the .NET standard library just before I stopped writing C# in 2020. So let's implement RPC in terms of MPSC channels!

In order to match the behavior of our channels and requests with Cap'n Proto C++, we're gonna need to lay a ground work for what's required here. Our channels are largely based on `ClientHook`. `ClientHook` has a few main operations:

* Send a request
* Await client resolution
* Identify the client
* Add a reference

And that's it! Requests are a bit more complicated though. With requests you can

* Await the response
* Create a promise pipelined client
* *Drop all references to the request to cancel it.*

Hmm. That sounds like it's gonna be a little harder.

## Immediate blockers

Initially, I tried using pre-existing types. For channels, I'd use a Tokio mpsc for requests and a tokio oneshot for request responses. Wrap the whole thing in an Arc, use an enum to describe all the different client types. But I hit some snags pretty quick:

Values can't remove themselves from an mpsc channel. Once they're in the channel they're stuck there forever until the value is read out of it. So that behavior is out.

*Allocations.* Every. Single. Thing. Does *something*. The MPSC channel is an allocation, that makes a linked list of allocations with all the requests. Each request has an allocation for the oneshot, but what if I want to share the response? Normally you'd use something like the `Shared` future from the `futures` crate, but *that's another allocation*. `Shared` just takes your future, awaits it, and puts the result in an `Arc`. Awful! We wanted to reduce allocations not increase them! Obviously this isn't going well.

To top it off, we can't optimize message forwarding! Forwarding operations are extremely common in RPC. It'd be great if we could forward from one mpsc channel to another without having to spawn a task to do it manually, especially because that'd mean **more allocations.**

## Custom types (the actual design doc)

Now that the backstory is out of the way, we can actually discuss the design of this library.

This library is based on 3 fundamental synchronization primitives:

* The "sharedshot"
* The request-response-pipeline request type
* The forwardable mpsc channel

These types are designed to be as abstract and generic as possible for testing, but they support all the features we need without explicitly referencing any Cap'n Proto types. This allows us to test things like drop behavior without having to get hacky with Cap'n Proto messages.

### sharedshot

The sharedshot is a synchronization primitive. It combines the `oneshot` with a `Shared` future, allowing you to have one-value many-receiver with one allocation. It's largely based on the implementation of the tokio `oneshot` with the `Notify` future. There's not much to say about its internals, except that the internals are designed in such a way that they can be reused by other types as we'll see later.

### request-response-pipeline

A request with a response and pipeline data all wrapped up in a single allocation. This combines a lot of behaviors into one data structure, but it's necessary for optimization and the implementation of some features. First let's talk about how this type is used.

Requests are created using a specific constructor. There are 3 constructors depending on what the user wants to do with their Cap'n Proto request:

* `request_response_pipeline`, the standard constructor. Returns a request, a response receiver, and a pipeline builder.
* `request_response`, returns a request and a response receiver. It does not allow for the creation of pipelined clients (since there's no builder.)
* `request_pipeline`, returns a request and a pipeline builder. It does not allow the response to be received.

These various constructors inform the request how it's going to be used. This can lead to certain optimizations down the line in the RPC system.

A request value is the root that allows you to operate on requests. You can pass a request to an mpsc channel to send it on that channel. If you receive the request from a channel, you can `respond` to the request to break it into it's components: the parameters of the request and the responder. The responder allows for 3 operations on the request:

* `tail_call`, which allows you to reuse the request object to make a new request.
* `set_pipeline`, which allows you to configure where pipelined clients go before the request is responded to.
* `respond`, which just accepts response data and immediately delivers it back to any receivers.

Requests also have the ability to tell if all response references have been dropped. Each type in the request types have a function called "finished" that returns a future which resolves when no more receivers are listening for the response. When a request is finished it will also attempt to remove itself from whatever mpsc channel it's in. This even works if an mpsc channel is forwarded to another.

Responses are also shared. You can share the mpsc channel response with as many receivers as you want. Each of them can await the result separately or you can clone the results reference and hand them out that way.

### Forwardable mpsc

A channel is a FIFO ordered list of requests and *events*. Generic events are necessary to support disembargos which must flow on the same path as requests do, but aren't really requests themselves.

This channel type is constructed around the following assumptions:

* There's lots of channels and there likely won't be a lot of contention any one in particular.
* Every channel mostly consists of requests.
* Every channel *will* resolve at some point into something else, or terminate with an error.

As such, the channel type is a mutex paired with a sharedshot. In the mutex is a linked list of requests and events as well as a waker. We assume there's lots of uncontested channels because that's largely what happens in a Cap'n Proto connection. A single capability will likely be used by one specific task or connection and multiple tasks are unlikely to be sending an extremely high volume of requests to a single capability in particular. The time spent holding the lock is also relatively short, as we're simply linking an already allocated node to a list.

The channel type does not use linked lists of arrays for value storage like tokio mpsc because requests are already big allocations, we can just link them together and save time and energy. Plus, when forwarding the channel to another, we can simply take the linked list of the first channel and attach it to the second channel! Now the forwarding operation is O(1).

Events are not an issue when it comes to allocations (each event will make a separate allocation to work with the linked list) because it's assumed events are not that common compared to requests.

It's assumed every channel will resolve, so we simply put a sharedshot in the channel and reuse it. There is only one channel that doesn't resolve and is considered "stable", that being local channels (that aren't waiting for a shorter path to exist).

The channel type acts like a normal mpsc channel. It has a `Sender` type with a `send` method, a `Recevier` type with a `recv` method, and a `close` method for `Receiver`. But it has some minor differences.

* `Sender::send()` returns a `Result` like a normal channel, with an `Err` case in case the channel was closed. But because channels can close with an error, this error is given along with the original request. `send` will also attempt to automatically *forward* the request to any resolved channels. Since request ordering is maintained within the channel, it will automatically go to the most resolved channel to place the request.

* `Sender` has methods to find what the channel resolved to.

* `Receiver::close` requires an error value to close the channel manually.

* `Receiver` has another function `forward_to` which consumes the channel and takes another `Sender` to forward all requests in the channel to. If the sender is determined to be the same as the `Receiver` channel, the `Receiver` is returned again in the `Result::Err` case to indicate that a circular resolution occured. Most of the time the receiver is just closed with an error saying as such.

## Local RPC (with work-stealing multi-threading)

With these types, implementing local RPC is a sinch. It's basically 40% of RPC! Just make an mpsc channel, wrap the sender in a typeless `Client` abstraction, wrap the server in a `Dispatcher` abstraction that sends requests to request handlers with suitable wrappers for call context. We can even easily implement blocking calls by just *not reading* from the channel when we have a blocking request that's being handled. Simple! No special code required. But we have another problem: Cap'n Proto messages themselves! While our synchronization primitives support multi-threading, our Cap'n Proto type readers and builders don't.

Cap'n Proto message readers and builders need interior mutability to function correctly! Readers need to be able to track how much of the message they've read while walking the message with shared references. They also need to be able to read from any arena including builder arenas. Builder arenas need to be accessable via a shared reference as well since you can carry multiple builders in the message at the same time (via orphans, which are detached from the tree).

Why does this matter? Well if we want our local servers to work with work-stealing, the futures they produce need to be `Send`. But readers and builders are *not* send. You need readers to read request parameters and builders to write the results.

Technically, this is only an issue if you carry a reader or builder across an await point, but what if a user does that! We need to support servers only existing on a single thread. But we don't want to lock people to that design. We want work-stealing! It's more limiting but if users want they can restrict themselves to only referencing the parameters or the results of the request synchronously. This way they can opt into work-stealing! But how do we expose both?

The answer is to make every method of a server synchronous, but allow the user to spawn off a task (either locally or globally) to handle async work. Each method accepts a `CallHandler` type with 4 functions for handling the call: either by immediately returning an error, handling the request synchronously, handling it async (with a `Send` future), or async (with a `!Send`) future.

## Remote RPC (with work-stealing multi-threading!)

Remote RPC is still a bit tough. It's a very big complicated state machine with a lot of moving parts. On one hand, you have outgoing messages from various imports and pipelined clients. On the other hand you have every single incoming request which can spawn it's own requests. Requests can come back that you have to optimize as tail calls over RPC, imports and exports have reference counts, and now third-party RPC exists! There's flow control, RPC systems, vat networks, idle optimizations(?), there's too many things!

Well, if you implement it that way there is...

### The System Abstraction

First we need to talk about `RpcSystem`. `RpcSystem` is the core type you interact with in C++ RPC to set up your... system. You give it a `VatNetwork` implementation and it drives the network doing tasks, connecting to vats, managing the existing connections, etc. But while we're here reconsidering *everything*, we should ask ourselves:

"*Is RpcSystem even a good abstraction?"*

No.

At least I don't think so.

Obviously `RpcSystem` is designed for a case where you have many vats you're connected to. Each one has it's own vat ID and the vat network implementation actually does *real work* figuring out the connections. But those cases are kinda rare? I truly believe most people nowadays are doing two-party RPC with one client and one server. Each side will only connect to the other for RPC. What does the `RpcSystem` even bring in this case? You get a loop that accepts connections that will never come, a map of connections which will only ever contain one. What's the point of this thing?

Now if you know the capnp library you know that `EzRpc` is designed for this case. You just want to do a two-party connection, here I did all the work for you! But `EzRpc` is just a wrapper around `RpcSystem`, a bad abstraction for how variable each network and RPC system will be.

`RpcSystem` does give a few nicities(?) like idle connections and flow control, but I keep seeing cases where people have to do their own custom thing on top of it and at this point I think we just need to conceed and realize `RpcSystem` is just not a good abstraction. Every network is different, let's just give people a basic connection that accepts some network as a parameter. Let the user build an `RpcSystem` for themselves for their use-case. This lets us implement `EzRpc` in a much simpler way and lets networks decide on different approaches to flow control, idle handling, etc.

recapn-rpc does not implement `RpcSystem`. It exposes a `Connection` type with a network parameter. It's the user's job deciding how they want to handle reading and writing messages for the connection. A two party RPC system like `EzRpc` will be provided, but a generic `RpcSystem` will not.

### Connection

The `Connection` is the primitive for remote RPC. It manages all the state of a single connection between yourself and another vat. The API is very simple:

* Get the bootstrap client with `bootstrap()`
* Put incoming messages into the connection with `handle_message`
* Get the current amount of words in flight with `call_words_in_flight()` (so users can implement flow control)
 * This might require a separate API to wait until the number of words in flight has dropped below a certain amount.
 * Originally this wasn't needed because you had to explicitly pump RPC messages through the system. So you could handle outgoing events until eventually the words in flight dropped below a certain level.
  * This was dropped to allow for work-stealing between the imports on the connection.

Most of the work on a connection is handling incoming messages and events. We can't really improve much here because there's only one pipe of incoming messages. However, there's a lot we can improve with outgoing messages. In a single-threaded connection, each import / promised client that sends messages out can block other messages from going out. Operations like message copying can eat up CPU and stall the whole connection. In a single threaded system, a single bad import on one connection can just freeze the whole system.

So in recapn-rpc, a `Connection` will spawn imports and promise pipelined clients to handle outgoing messages on separate tasks. This way, if a single import is backing up and handling a lot of work like proxying, it doesn't freeze the whole RPC system. Events that the import needs to handle relative to other requests are sent on it's mpsc channel in order to keep message ordering.