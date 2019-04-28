import React, { useState, useEffect } from 'react'
import {
  Segment,
  Dimmer,
  Loader,
  Label,
  Table,
  Checkbox,
  Button,
  Pagination,
  Grid,
  Modal,
  Dropdown
} from 'semantic-ui-react'
import uuid from 'uuid/v4'
import 'style/data-table.scss'

/* global fetch */

const rowsPerPageOptions = [
  {
    text: 25,
    value: 25
  },
  {
    text: 50,
    value: 50
  },
  {
    text: 100,
    value: 100
  },
  {
    text: 200,
    value: 200
  }
]

function DataTable () {
  const [loading, setLoading] = useState(true)
  const [pagination, setPagination] = useState({
    page: 1,
    of: 1,
    offset: 0,
    rows: 50
  })
  const [data, setData] = useState(null)
  const [selection, setSelection] = useState({})
  const [deleteModal, setDeleteModal] = useState(false)

  async function getData (page) {
    let newPagination = { ...pagination }
    if (page > newPagination.page) {
      while (page > newPagination.page) {
        newPagination.offset += newPagination.rows
        newPagination.page += 1
      }
    } else {
      while (page < newPagination.page) {
        newPagination.offset -= newPagination.rows
        newPagination.page -= 1
      }
    }

    let res = null
    try {
      res = await fetch(
        `api/services?offset=${newPagination.offset}&rows=${newPagination.rows}`
      ).then((res) => res.json())
    } catch (ex) { /* Intentionally left blank */ }

    setLoading(false)

    if (res === null) {
      setData(null)
    } else {
      setData({
        headers: Object.keys(res.services[0]),
        rows: res.services.map((row) => Object.values(row))
      })

      if (res.rows_count > newPagination.rows) {
        newPagination.of = res.rows_count / newPagination.rows
        if (newPagination.of % 1 !== 0) {
          newPagination.of = parseInt(newPagination.of) + 1
        }
      } else {
        newPagination.of = 1
      }
      setPagination(newPagination)
    }
  }

  function toggleSelection (id) {
    let newSelection = { ...selection }
    if (newSelection[id]) {
      delete newSelection[id]
    } else {
      newSelection[id] = true
    }
    setSelection(newSelection)
  }

  function toggleAll (action) {
    let newSelection = {}
    for (let row of data.rows) {
      switch (action) {
        case 'select':
          newSelection[row[0]] = true
          break
        case 'deselect':
          delete newSelection[row[0]]
          break
      }
    }
    setSelection(newSelection)
  }

  async function deleteRecords () {
    let IDs = Object.keys(selection)
      .map((el) => parseInt(el))

    let res = null
    try {
      res = await fetch('api/services',
        {
          method: 'DELETE',
          headers: {
            'Accept': 'application/json',
            'Content-Type': 'application/json'
          },
          body: JSON.stringify(IDs)
        })
    } catch (ex) { /* Intentionally left blank */ }

    setLoading(false)

    if (res === null) {
      setData(null)
    } else {
      let newData = JSON.parse(JSON.stringify(data))
      for (let r in newData.rows) {
        for (let id of IDs) {
          if (newData.rows[r][0] === parseInt(id)) {
            newData.rows.splice(r, 1)
          }
        }
      }
      setData(newData)
      setSelection({})

      if (newData.rows.length === 0) {
        if (pagination.page === 1 || pagination.page !== pagination.of) {
          getData(pagination.page)
        } else {
          getData(pagination.page - 1)
        }
      }
    }
  }

  useEffect(() => {
    getData(pagination.page)
  }, [pagination.rows])

  if (loading) {
    return (
      <div className='data-table'>
        <Segment>
          <Dimmer active inverted>
            <Loader size='massive' />
          </Dimmer>
        </Segment>
      </div>
    )
  }

  if (data === null) {
    return <p>Fetch error</p>
  }

  return (
    <div className='data-table'>
      <Table celled>
        <Table.Header>
          <Table.Row>
            <Table.HeaderCell />
            {
              data.headers.map((el) => {
                return <Table.HeaderCell key={uuid()}>{el}</Table.HeaderCell>
              })
            }
          </Table.Row>
        </Table.Header>
        <Table.Body>
          {
            data.rows.map((fields) => {
              let cells = []
              for (let field of fields) {
                cells.push(<Table.Cell key={uuid()}><Label>{field}</Label></Table.Cell>)
              }
              return (
                <Table.Row key={fields[0]}>
                  <Table.Cell collapsing>
                    <Checkbox
                      checked={selection[fields[0]] === true}
                      onChange={(e) => toggleSelection(fields[0])} />
                  </Table.Cell>
                  {cells}
                </Table.Row>
              )
            })
          }
        </Table.Body>
        <Table.Footer>
          <Table.Row>
            <Table.HeaderCell colSpan='9'>
              <Grid>
                <Grid.Row>
                  <Grid.Column width={4}>
                    <Button
                      onClick={(e) => toggleAll('select')}>
                      Select All
                    </Button>
                    <Button
                      onClick={(e) => toggleAll('deselect')}>
                      Deselect All
                    </Button>
                  </Grid.Column>
                  <Grid.Column width={10}>
                    <Pagination
                      size='tiny'
                      activePage={pagination.page}
                      totalPages={pagination.of}
                      onPageChange={(e, { activePage }) => getData(activePage)} />
                    <Dropdown
                      className='dropdown'
                      placeholder='Rows'
                      compact
                      selection
                      defaultValue={pagination.rows}
                      options={rowsPerPageOptions}
                      onChange={(e, { text, value }) => setPagination({
                        ...pagination,
                        rows: value
                      })} />
                  </Grid.Column>
                  <Grid.Column width={2}>
                    <Button
                      floated='right'
                      disabled={Object.keys(selection).length === 0}
                      onClick={(e) => setDeleteModal(!deleteModal)}>
                      Delete
                    </Button>
                    <Modal
                      open={deleteModal}
                      closeOnEscape={false}
                      closeOnDimmerClick={false}
                      onClose={(e) => setDeleteModal(!deleteModal)} >
                      <Modal.Header>Delete selected rows</Modal.Header>
                      <Modal.Content>
                        <p>Are you sure?</p>
                      </Modal.Content>
                      <Modal.Actions>
                        <Button onClick={(e) => setDeleteModal(!deleteModal)} negative>
                          No
                        </Button>
                        <Button
                          onClick={(e) => {
                            deleteRecords()
                            setDeleteModal(!deleteModal)
                          }}
                          positive
                          labelPosition='right'
                          icon='checkmark'
                          content='Yes' />
                      </Modal.Actions>
                    </Modal>
                  </Grid.Column>
                </Grid.Row>
              </Grid>
            </Table.HeaderCell>
          </Table.Row>
        </Table.Footer>
      </Table>
    </div>
  )
}

export default DataTable
