participant Scene (client) as C
participant Kernel (server) as S
C->S: Request {procedure_id, payload}
S->C: Response {message_id, payload}
