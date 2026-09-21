enum Status {
    Pending = "pending",
    Active = "active",
    Done = "done",
}

const statuses: { Active: Status; Done: Status; Pending: Status } = Status;
