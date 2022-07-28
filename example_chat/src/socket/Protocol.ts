
export type Packet =
{
    type: 'set-name',
    name: string,
} |
{
    type: 'message',
    mex: string,
};
