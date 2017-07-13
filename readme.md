# Protocol

A message consists of a UTF8 string, prepended by a byte indicating its length (see the `Codec` in `protocol/src/async.rs`)

The client works as follows (see `client`):
* Upon connecting, send a message to the server stating the user's nickname
* For each line from stdin, send it to the server as a message
* Receive messages from the server and print them as lines to stdout

The server works as follows (see `server`), for each connection:
* Receive the nickname
* For each received chat message, prepend the nickname of the user to it and broadcast it to other connected users.
